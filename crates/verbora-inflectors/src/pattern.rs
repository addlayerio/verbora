//! Translating the reference regular expressions into `regex`-crate patterns.
//!
//! Every inflection rule in the reference is a reference `RegExp`. Handing those
//! source strings straight to [`regex::Regex`] compiles — and silently changes
//! what they match. Three differences bite immediately, and all three are
//! reachable from ordinary text:
//!
//! | Construct | the reference (no `/u`) | `regex` crate |
//! |---|---|---|
//! | `.` | excludes `\n` `\r` `U+2028` `U+2029` | excludes `\n` only |
//! | `/i` on `s` | does **not** match `ſ` (U+017F) | matches it |
//! | `/i` on `k` | does **not** match `K` (U+212A) | matches it |
//!
//! The `.` row is not hypothetical: `NounInflector.pluralize("ab\rcd")` is
//! `"abs\rcd"` in the reference, because `(.*)` stops at the carriage return. A
//! port that leaves `.` alone produces `"ab\rcds"`.
//!
//! The `/i` rows come from *legacy* (non-`/u`) canonicalisation, which
//! The reference defines as "uppercase the character; if that yields more than one
//! character, or maps a non-ASCII character into ASCII, leave it alone". The
//! `regex` crate instead uses Unicode *simple case folding*, under which `ſ`
//! folds to `s` and `K` folds to `k`. So `/(x|ch|ss|sh|s|z)$/i` fires on `maſ`
//! in Rust but not in the reference — `pluralize("maſ")` is `"maſs"`, not
//! `"maſes"`.
//!
//! Rather than paper over this per rule, [`translate`] rewrites the pattern:
//! `.` becomes an explicit negated class, and every case-insensitive literal
//! becomes an explicit two- or three-member class computed from the reference's own
//! `Canonicalize`. The result is a pattern with `(?i)` *never enabled*, so the
//! `regex` crate's folding tables cannot participate at all.
//!
//! # Supported subset
//!
//! Enough for every rule in the reference, plus the common shapes a caller of
//! [`crate::Rule::from_pattern`] would write: literals, `.`, `^`, `$`, `|`,
//! quantifiers (`*` `+` `?` `{n,m}` and their lazy forms), groups (`(`, `(?:`,
//! `(?<name>`), character classes with ranges and negation, and the escapes
//! `\d \D \w \W \s \S \b \B \0 \n \r \t \v \f \xHH \uHHHH \u{…}` plus escaped
//! punctuation.
//!
//! Deliberately rejected, because they cannot be expressed and a silent
//! mistranslation is worse than an error: lookahead, lookbehind, backreferences
//! and `\p{…}`. The one rule in the reference that needs a lookahead —
//! `/^(?!talis|.*hu)(.*)man$/i` — is hand-written instead; see
//! [`crate::Rule`].

use std::fmt;

use regex::Regex;

/// What `.` means in a reference regular expression without the `/s` flag.
///
/// The reference excludes all four line terminators; the `regex` crate's `.`
/// excludes only `\n`. Spelling the class out is the whole fix.
const DOT: &str = r"[^\n\r\x{2028}\x{2029}]";

/// The members of the reference's `\s`: `WhiteSpace` plus `LineTerminator`.
///
/// This differs from the `regex` crate's `\s` (Unicode `White_Space`) in both
/// directions: the reference includes `U+FEFF` and excludes `U+0085`. The same
/// asymmetry is documented for tokenizers in [`verbora_core::is_whitespace`].
const SPACE: &str = r"\t\n\x{b}\x{c}\r \x{a0}\x{1680}\x{2000}-\x{200a}\x{2028}\x{2029}\x{202f}\x{205f}\x{3000}\x{feff}";

/// The reference's `\w`, which is ASCII-only — unlike the `regex` crate's.
const WORD: &str = "0-9A-Za-z_";

/// The reference's `\d`, likewise ASCII-only.
const DIGIT: &str = "0-9";

/// A reference pattern that could not be translated.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PatternError {
    message: String,
}

impl PatternError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }

    /// The reason translation or compilation failed.
    pub fn message(&self) -> &str {
        &self.message
    }
}

impl fmt::Display for PatternError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for PatternError {}

/// Compiles a reference pattern into a [`Regex`] with the reference semantics.
///
/// `source` is the text of a reference `RegExp` — what `RegExp.prototype.source`
/// returns — and `ignore_case` is its `/i` flag. The `/g` flag has no analogue
/// here: the inflectors substitute only the first match either way, which is
/// what the non-global reference patterns do.
///
/// # Errors
///
/// Returns [`PatternError`] if the pattern uses a construct outside the
/// supported subset (see the module docs) or if the translated pattern is
/// rejected by the `regex` crate.
pub fn compile(source: &str, ignore_case: bool) -> Result<Regex, PatternError> {
    let translated = translate(source, ignore_case)?;
    Regex::new(&translated).map_err(|e| {
        PatternError::new(format!(
            "reference pattern /{source}/{} translated to {translated:?}, which the regex crate rejected: {e}",
            if ignore_case { "i" } else { "" }
        ))
    })
}

/// Translates a reference pattern into `regex`-crate syntax.
///
/// Exposed separately from [`compile`] so a caller can inspect (or test) the
/// rewriting without building a regex.
///
/// # Errors
///
/// Returns [`PatternError`] for constructs outside the supported subset.
pub fn translate(source: &str, ignore_case: bool) -> Result<String, PatternError> {
    let src: Vec<char> = source.chars().collect();
    // Case classes roughly double a literal, and `.` expands ~20x; this is a
    // cheap over-estimate that avoids regrowth on realistic patterns.
    let mut out = String::with_capacity(source.len() * 3);
    let mut i = 0;

    while i < src.len() {
        match src[i] {
            '.' => {
                out.push_str(DOT);
                i += 1;
            }
            '[' => i = translate_class(&src, i, ignore_case, &mut out)?,
            '\\' => i = translate_escape(&src, i, ignore_case, false, &mut out)?,
            '(' => i = translate_group_open(&src, i, &mut out)?,
            '^' | '$' | '|' | ')' | '*' | '+' | '?' => {
                out.push(src[i]);
                i += 1;
            }
            '{' => i = translate_brace(&src, i, &mut out),
            c => {
                push_literal(&mut out, c, ignore_case);
                i += 1;
            }
        }
    }

    Ok(out)
}

// ---------------------------------------------------------------------------
// Case handling
// ---------------------------------------------------------------------------

/// The reference's `Canonicalize` for a non-`/u` regular expression.
///
/// From the reference language grammar: uppercase the character; if the uppercase
/// mapping is not exactly one character (`ß` → `SS`), keep the original; if it
/// maps a non-ASCII character into ASCII (`ſ` → `S`, `ı` → `I`), also keep the
/// original. Two characters match under `/i` exactly when they canonicalise to
/// the same value.
///
/// Implementing this as a literal round trip — rather than reaching for
/// [`char::is_uppercase`] or the `regex` crate's folding — is what keeps `ſ`,
/// `K`, `Å` and `ı` behaving as the reference does.
fn canonicalize(c: char) -> char {
    let mut upper = c.to_uppercase();
    let first = upper
        .next()
        .expect("char::to_uppercase always yields at least one character");
    if upper.next().is_some() {
        // Multi-character uppercase mapping: the reference leaves the char alone.
        return c;
    }
    if !c.is_ascii() && first.is_ascii() {
        // Non-ASCII must not be canonicalised into ASCII.
        return c;
    }
    first
}

/// The one character a case mapping yields, or `None` if it yields several.
fn single(mut mapping: impl Iterator<Item = char>) -> Option<char> {
    let first = mapping.next()?;
    mapping.next().is_none().then_some(first)
}

/// Calls `f` for every character that matches `c` under the reference's `/i`.
///
/// The candidate set is `{c, lowercase(c), uppercase(c)}` filtered by equal
/// canonicalisation, which is exact for every character with a simple case
/// mapping and correctly yields the singleton `{c}` for `ſ`, `K`, `ı`, digits,
/// punctuation and CJK.
fn for_each_case_variant(c: char, mut f: impl FnMut(char)) {
    f(c);
    let target = canonicalize(c);
    let mut emitted = c;
    for candidate in [single(c.to_lowercase()), single(c.to_uppercase())]
        .into_iter()
        .flatten()
    {
        if candidate != c && candidate != emitted && canonicalize(candidate) == target {
            f(candidate);
            emitted = candidate;
        }
    }
}

/// Emits one literal character, as a case class when `/i` is in effect.
fn push_literal(out: &mut String, c: char, ignore_case: bool) {
    if !ignore_case {
        push_escaped(out, c);
        return;
    }
    let mut variants = String::new();
    let mut count = 0;
    for_each_case_variant(c, |v| {
        count += 1;
        push_class_escaped(&mut variants, v);
    });
    if count == 1 {
        push_escaped(out, c);
    } else {
        out.push('[');
        out.push_str(&variants);
        out.push(']');
    }
}

/// Emits one character-class member, expanded for `/i` when needed.
fn push_class_member(out: &mut String, c: char, ignore_case: bool) {
    if ignore_case {
        for_each_case_variant(c, |v| push_class_escaped(out, v));
    } else {
        push_class_escaped(out, c);
    }
}

/// Whether a character must be escaped to appear literally outside a class.
const fn is_meta(c: char) -> bool {
    matches!(
        c,
        '\\' | '.'
            | '+'
            | '*'
            | '?'
            | '('
            | ')'
            | '|'
            | '['
            | ']'
            | '{'
            | '}'
            | '^'
            | '$'
            | '#'
            | '&'
            | '-'
            | '~'
    )
}

/// Appends `c` as a literal, escaping metacharacters and control codes.
fn push_escaped(out: &mut String, c: char) {
    if is_control(c) {
        push_hex(out, c);
    } else if is_meta(c) {
        out.push('\\');
        out.push(c);
    } else {
        out.push(c);
    }
}

/// Appends `c` as a character-class member, escaping what a class treats
/// specially — including `&`, `-` and `~`, which the `regex` crate uses for set
/// operations when doubled.
fn push_class_escaped(out: &mut String, c: char) {
    if is_control(c) {
        push_hex(out, c);
    } else if matches!(c, '\\' | ']' | '[' | '^' | '-' | '&' | '~') {
        out.push('\\');
        out.push(c);
    } else {
        out.push(c);
    }
}

/// Control characters are written as `\x{…}` so the pattern stays printable and
/// unambiguous in error messages.
const fn is_control(c: char) -> bool {
    (c as u32) < 0x20 || c as u32 == 0x7f
}

fn push_hex(out: &mut String, c: char) {
    out.push_str("\\x{");
    // `write!` on a String cannot fail, but returns Result; format the digits
    // by hand to keep this infallible and allocation-free.
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let v = c as u32;
    let mut started = false;
    for shift in (0..8).rev() {
        let nibble = (v >> (shift * 4)) & 0xf;
        if nibble != 0 || started || shift == 0 {
            started = true;
            out.push(HEX[nibble as usize] as char);
        }
    }
    out.push('}');
}

// ---------------------------------------------------------------------------
// Sub-scanners
// ---------------------------------------------------------------------------

/// Translates `(`, `(?:`, `(?<name>`; rejects lookaround.
fn translate_group_open(src: &[char], i: usize, out: &mut String) -> Result<usize, PatternError> {
    if src.get(i + 1) != Some(&'?') {
        out.push('(');
        return Ok(i + 1);
    }
    match src.get(i + 2) {
        Some(':') => {
            out.push_str("(?:");
            Ok(i + 3)
        }
        Some('=') | Some('!') => Err(PatternError::new(
            "lookahead is not supported: the regex crate cannot express it, and \
             an approximation would silently change which rule fires",
        )),
        Some('<') if matches!(src.get(i + 3), Some('=') | Some('!')) => Err(PatternError::new(
            "lookbehind is not supported: the regex crate cannot express it",
        )),
        Some('<') => {
            // Named group. Emitted in the `(?P<name>` form, which every regex
            // version accepts.
            out.push_str("(?P<");
            let mut j = i + 3;
            while let Some(&c) = src.get(j) {
                if c == '>' {
                    out.push('>');
                    return Ok(j + 1);
                }
                out.push(c);
                j += 1;
            }
            Err(PatternError::new("unterminated named group"))
        }
        _ => Err(PatternError::new(
            "unsupported group modifier after `(?`; only `(?:` and `(?<name>` are supported",
        )),
    }
}

/// Copies a `{n}`, `{n,}` or `{n,m}` repetition verbatim; a `{` that is not one
/// is a literal in the reference (Annex B) and is escaped so the `regex` crate
/// agrees.
fn translate_brace(src: &[char], i: usize, out: &mut String) -> usize {
    let mut j = i + 1;
    let mut digits = 0;
    while matches!(src.get(j), Some(c) if c.is_ascii_digit()) {
        j += 1;
        digits += 1;
    }
    if digits > 0 {
        if src.get(j) == Some(&',') {
            j += 1;
            while matches!(src.get(j), Some(c) if c.is_ascii_digit()) {
                j += 1;
            }
        }
        if src.get(j) == Some(&'}') {
            for &c in &src[i..=j] {
                out.push(c);
            }
            return j + 1;
        }
    }
    out.push_str("\\{");
    i + 1
}

/// Translates a character class, expanding members for `/i`.
fn translate_class(
    src: &[char],
    start: usize,
    ignore_case: bool,
    out: &mut String,
) -> Result<usize, PatternError> {
    let mut i = start + 1;
    let mut body = String::new();
    let negated = src.get(i) == Some(&'^');
    if negated {
        i += 1;
    }

    let mut empty = true;
    while let Some(&c) = src.get(i) {
        match c {
            ']' => {
                if empty {
                    // The reference's `[]` matches nothing and `[^]` matches
                    // everything; the regex crate rejects both spellings.
                    return Err(PatternError::new("empty character class is not supported"));
                }
                out.push('[');
                if negated {
                    out.push('^');
                }
                out.push_str(&body);
                out.push(']');
                return Ok(i + 1);
            }
            '\\' => {
                i = translate_escape(src, i, ignore_case, true, &mut body)?;
                empty = false;
            }
            _ => {
                // A range needs both endpoints expanded together, so peek.
                let is_range =
                    src.get(i + 1) == Some(&'-') && matches!(src.get(i + 2), Some(&e) if e != ']');
                if is_range {
                    let end = src[i + 2];
                    push_range(&mut body, c, end, ignore_case)?;
                    i += 3;
                } else {
                    push_class_member(&mut body, c, ignore_case);
                    i += 1;
                }
                empty = false;
            }
        }
    }

    Err(PatternError::new("unterminated character class"))
}

/// Emits `lo-hi`, plus the opposite-case range when `/i` is in effect.
///
/// Restricted to ASCII endpoints under `/i`: for a non-ASCII range the set of
/// characters that canonicalise into it is not a range, and emitting the range
/// alone would silently under-match. Refusing is the honest option.
fn push_range(out: &mut String, lo: char, hi: char, ignore_case: bool) -> Result<(), PatternError> {
    if lo > hi {
        return Err(PatternError::new(format!(
            "character class range out of order: {lo:?}-{hi:?}"
        )));
    }
    push_class_escaped(out, lo);
    out.push('-');
    push_class_escaped(out, hi);

    if !ignore_case {
        return Ok(());
    }
    if !lo.is_ascii() || !hi.is_ascii() {
        return Err(PatternError::new(format!(
            "case-insensitive non-ASCII range {lo:?}-{hi:?} is not supported: the \
             set of characters matching it is not itself a range"
        )));
    }
    // Only letters have an ASCII case partner; overlap with the original range
    // is harmless in a class.
    let swap = |c: char| {
        if c.is_ascii_lowercase() {
            c.to_ascii_uppercase()
        } else {
            c.to_ascii_lowercase()
        }
    };
    if lo.is_ascii_alphabetic()
        && hi.is_ascii_alphabetic()
        && lo.is_ascii_lowercase() == hi.is_ascii_lowercase()
    {
        push_class_escaped(out, swap(lo));
        out.push('-');
        push_class_escaped(out, swap(hi));
    }
    Ok(())
}

/// Translates one backslash escape. `in_class` selects the class-member form,
/// which differs for `\b` (backspace inside a class, word boundary outside).
fn translate_escape(
    src: &[char],
    i: usize,
    ignore_case: bool,
    in_class: bool,
    out: &mut String,
) -> Result<usize, PatternError> {
    let Some(&c) = src.get(i + 1) else {
        return Err(PatternError::new("pattern ends with a backslash"));
    };

    /// Wraps class members in brackets when they appear outside a class.
    fn emit_set(out: &mut String, members: &str, negate: bool, in_class: bool) {
        if in_class && !negate {
            out.push_str(members);
        } else {
            out.push('[');
            if negate {
                out.push('^');
            }
            out.push_str(members);
            out.push(']');
        }
    }

    match c {
        'd' => emit_set(out, DIGIT, false, in_class),
        'D' => emit_set(out, DIGIT, true, in_class),
        'w' => emit_set(out, WORD, false, in_class),
        'W' => emit_set(out, WORD, true, in_class),
        's' => emit_set(out, SPACE, false, in_class),
        'S' => emit_set(out, SPACE, true, in_class),
        'b' if in_class => out.push_str("\\x{8}"),
        // `(?-u:)` selects the ASCII word-boundary definition, matching
        // the reference's `\b`, which is defined over `[A-Za-z0-9_]` only.
        'b' => out.push_str("(?-u:\\b)"),
        'B' if in_class => {
            return Err(PatternError::new(
                "\\B is not valid inside a character class",
            ));
        }
        'B' => out.push_str("(?-u:\\B)"),
        'n' | 'r' | 't' | 'f' | 'v' | '0' => {
            let ch = match c {
                'n' => '\n',
                'r' => '\r',
                't' => '\t',
                'f' => '\u{c}',
                'v' => '\u{b}',
                _ => '\0',
            };
            if c == '0' && matches!(src.get(i + 2), Some(d) if d.is_ascii_digit()) {
                return Err(PatternError::new("legacy octal escapes are not supported"));
            }
            if in_class {
                push_class_member(out, ch, ignore_case);
            } else {
                push_literal(out, ch, ignore_case);
            }
        }
        'x' | 'u' => {
            let (ch, next) = parse_code_point(src, i, c)?;
            if in_class {
                push_class_member(out, ch, ignore_case);
            } else {
                push_literal(out, ch, ignore_case);
            }
            return Ok(next);
        }
        'c' => {
            let Some(&letter) = src.get(i + 2).filter(|l| l.is_ascii_alphabetic()) else {
                return Err(PatternError::new("\\c must be followed by a letter"));
            };
            let ch = char::from(letter.to_ascii_uppercase() as u8 - b'A' + 1);
            if in_class {
                push_class_member(out, ch, ignore_case);
            } else {
                push_literal(out, ch, ignore_case);
            }
            return Ok(i + 3);
        }
        '1'..='9' => {
            return Err(PatternError::new(
                "backreferences are not supported: the regex crate cannot express them",
            ));
        }
        'k' => {
            return Err(PatternError::new(
                "named backreferences are not supported: the regex crate cannot express them",
            ));
        }
        'p' | 'P' => {
            return Err(PatternError::new(
                "\\p{…} is not supported: in a non-unicode reference pattern it is an \
                 identity escape, and in a unicode one the property sets differ",
            ));
        }
        other => {
            // Identity escape: `\.`, `\/`, `\$`, and (per Annex B) anything else.
            if in_class {
                push_class_member(out, other, ignore_case);
            } else {
                push_literal(out, other, ignore_case);
            }
        }
    }

    Ok(i + 2)
}

/// Parses `\xHH`, `\uHHHH` or `\u{…}`, returning the character and the index
/// just past it.
fn parse_code_point(src: &[char], i: usize, kind: char) -> Result<(char, usize), PatternError> {
    let (start, len, end) = if kind == 'x' {
        (i + 2, 2, i + 4)
    } else if src.get(i + 2) == Some(&'{') {
        let mut j = i + 3;
        while matches!(src.get(j), Some(c) if *c != '}') {
            j += 1;
        }
        if src.get(j) != Some(&'}') {
            return Err(PatternError::new("unterminated \\u{…} escape"));
        }
        (i + 3, j - (i + 3), j + 1)
    } else {
        (i + 2, 4, i + 6)
    };

    if start + len > src.len() {
        return Err(PatternError::new("truncated character escape"));
    }
    let digits: String = src[start..start + len].iter().collect();
    let value = u32::from_str_radix(&digits, 16)
        .map_err(|_| PatternError::new(format!("invalid hex escape {digits:?}")))?;
    let ch = char::from_u32(value).ok_or_else(|| {
        PatternError::new(format!(
            "escape \\u{value:04x} denotes a lone surrogate, which a Rust string cannot hold"
        ))
    })?;
    Ok((ch, end))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dot_excludes_all_four_reference_line_terminators() {
        let re = compile("^(.*)$", true).unwrap();
        assert!(re.is_match("abc"));
        for terminator in ["\n", "\r", "\u{2028}", "\u{2029}"] {
            assert!(
                !re.is_match(&format!("a{terminator}b")),
                "`.` must not match {terminator:?}"
            );
        }
        // A tab is not a line terminator and must still match.
        assert!(re.is_match("a\tb"));
    }

    #[test]
    fn ignore_case_uses_reference_canonicalization_not_unicode_folding() {
        let re = compile("s$", true).unwrap();
        assert!(re.is_match("as"));
        assert!(re.is_match("aS"));
        // U+017F LATIN SMALL LETTER LONG S folds to `s` under Unicode simple
        // folding, which is what `(?i)` would use — the reference does not.
        assert!(!re.is_match("a\u{17f}"));

        let k = compile("k", true).unwrap();
        assert!(k.is_match("K"));
        assert!(!k.is_match("\u{212a}"), "Kelvin sign must not match /k/i");

        let a = compile("a", true).unwrap();
        assert!(!a.is_match("\u{212b}"), "angstrom sign must not match /a/i");

        // `ı` uppercases to ASCII `I`, so the reference refuses the mapping.
        let i = compile("i", true).unwrap();
        assert!(!i.is_match("\u{131}"));
        assert!(!i.is_match("\u{130}"));
    }

    #[test]
    fn accented_latin_still_folds() {
        let re = compile("^(caf)é$", true).unwrap();
        assert!(re.is_match("café"));
        assert!(re.is_match("CAFÉ"));
        assert!(!re.is_match("cafe"));
    }

    #[test]
    fn pipe_inside_a_class_is_a_literal_member() {
        // The reference really does write `[^a|e|i|o|u]`, which excludes the
        // pipe character as well as the vowels.
        let re = compile("([^a|e|i|o|u])y$", true).unwrap();
        assert!(re.is_match("try"));
        assert!(!re.is_match("ay"));
        assert!(!re.is_match("AY"));
        assert!(!re.is_match("a|y".get(1..).unwrap()));
    }

    #[test]
    fn anchors_are_not_multiline() {
        let re = compile("^ab$", true).unwrap();
        assert!(re.is_match("ab"));
        assert!(!re.is_match("x\nab"));
        assert!(!re.is_match("ab\n"));
    }

    #[test]
    fn dead_anchor_rule_compiles_and_never_matches() {
        // `/$zz/i` from present_verb_inflector: `$` then literals.
        let re = compile("$zz", true).unwrap();
        assert!(!re.is_match("buzz"));
        assert!(!re.is_match(""));
    }

    #[test]
    fn zero_width_end_anchor_matches_empty_at_end() {
        let re = compile("$", true).unwrap();
        let m = re.find("abc").unwrap();
        assert_eq!((m.start(), m.end()), (3, 3));
    }

    #[test]
    fn escapes_and_classes() {
        assert!(compile(r"\d+", false).unwrap().is_match("42"));
        // The reference's `\d` is ASCII-only; the regex crate's is Unicode Nd.
        assert!(!compile(r"^\d$", false).unwrap().is_match("٣"));
        assert!(compile(r"^\s$", false).unwrap().is_match("\u{feff}"));
        assert!(!compile(r"^\s$", false).unwrap().is_match("\u{85}"));
        assert!(compile(r"^[a-c]+$", true).unwrap().is_match("aBc"));
        assert!(!compile(r"^[a-c]+$", true).unwrap().is_match("d"));
        assert!(compile(r"^\x41$", true).unwrap().is_match("a"));
        assert!(compile(r"^\u0041$", true).unwrap().is_match("A"));
    }

    #[test]
    fn unsupported_constructs_are_rejected_loudly() {
        for pattern in [
            "^(?!talis)x",
            "(?=a)b",
            "(?<=a)b",
            r"(a)\1",
            r"\p{L}",
            r"a\",
            "[",
            "[]",
        ] {
            assert!(
                translate(pattern, true).is_err(),
                "{pattern:?} should not translate"
            );
        }
    }

    #[test]
    fn alternation_is_ordered_the_way_the_reference_orders_it() {
        // The reference relies on ordered alternation in `(x|ch|ss|sh|s|z)$`,
        // `(au|eau|eu|œu)$` and a dozen French rules. The `regex` crate's default
        // is leftmost-*first*, the same as the reference — not the leftmost-longest
        // of POSIX — so the alternatives can be copied in source order. Pinning
        // it here because a switch to POSIX semantics would change which
        // alternative fires without changing any pattern.
        let re = compile("a|ab", false).unwrap();
        assert_eq!(re.find("ab").unwrap().as_str(), "a");

        let french = compile("(au|eau|eu|œu)$", true).unwrap();
        assert_eq!(french.find("cadeau").unwrap().start(), 3);
    }

    #[test]
    fn canonicalize_matches_the_reference_definition() {
        assert_eq!(canonicalize('a'), 'A');
        assert_eq!(canonicalize('A'), 'A');
        assert_eq!(canonicalize('é'), 'É');
        // Multi-character uppercase mapping: unchanged.
        assert_eq!(canonicalize('ß'), 'ß');
        // Non-ASCII must not canonicalise into ASCII.
        assert_eq!(canonicalize('\u{17f}'), '\u{17f}');
        assert_eq!(canonicalize('\u{131}'), '\u{131}');
        // Unchanged for everything caseless.
        assert_eq!(canonicalize('7'), '7');
        assert_eq!(canonicalize('日'), '日');
        assert_eq!(canonicalize('😀'), '😀');
    }
}
