//! `TF_Parser`: the grammar that turns a rule string into a transformation rule.
//!
//! # Why this is hand-written
//!
//! The reference is `lib/TF_Parser`, 732 lines emitted by a PEG parser generator (0.10.0) from a
//! 30-line grammar. Porting the *generated* parser would mean porting that generator's
//! runtime — a packrat cache, a failure-position tracker, an expectation
//! accumulator — none of which has any bearing on the contract. The grammar
//! itself has no recursion and exactly one ordered choice, so a direct scanner
//! reproduces it in a fraction of the code:
//!
//! ```text
//! transformation_rule = category1 identifier identifier identifier*
//! category1  = wild_card / identifier
//! wild_card  = "*" S_no_eol
//! identifier = [!-~\xA1-\xFF]+ S_no_eol
//! S_no_eol   = (" " / "\t" / Comment)*
//! Comment    = "//" (!EOL .)* (EOL / EOI)
//! EOL        = "\r\n" / "\n" / "\r"
//! ```
//!
//! What *is* ported literally is the generated parser's error reporting, because the thrown
//! message is observable and the fixture records twenty-three distinct ones. The
//! scanner therefore keeps the same furthest-failure position and expectation
//! set the generated parser does, and formats them with the generated parser's own rules
//! (descriptions sorted, then deduplicated, then joined with `, ` and `or`).
//!
//! # What a naive splitter gets wrong
//!
//! Splitting on whitespace looks equivalent and is not:
//!
//! | Input | This grammar | `split_whitespace` |
//! |---|---|---|
//! | `" A B C"` | **error** — the start rule has no leading `S` | accepted |
//! | `"// only comment"` | `('//', 'only', 'comment')` — `/` is an identifier char | a comment |
//! | `"*A B C"` | `('*', 'A', 'B', 'C')` — the wildcard is one character | `('*A', 'B', 'C')` |
//! | `"A B C d e f"` | **both** parameters dropped | keeps `d` and `e` |
//! | `"A B C\n"` | error — `S_no_eol` excludes newlines | accepted |
//! | `"A😀 B C"` | error — the class is one UTF-16 code unit wide | accepted |

use std::fmt;

/// A position in the input, in UTF-16 code units.
///
/// The generated parser counts `charAt` steps, so an astral character advances `offset`
/// and `column` by two. Lines are 1-based and advance on `\n` only — a lone
/// `\r` does not start a new line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Position {
    /// Zero-based offset in UTF-16 code units.
    pub offset: usize,
    /// One-based line number.
    pub line: usize,
    /// One-based column, in UTF-16 code units.
    pub column: usize,
}

/// The span a syntax error covers: one character, or an empty span at the end.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Location {
    /// Where the offending character starts.
    pub start: Position,
    /// Where it ends; equal to `start` at end of input.
    pub end: Position,
}

/// What the parser was willing to accept at the failure position.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Expectation {
    /// A literal string, e.g. `"*"` or `"//"`.
    Literal(&'static str),
    /// The identifier character class, printed as `[!-~¡-ÿ]`.
    ///
    /// The grammar spells the upper range `\xA1-\xFF`, but the generator's `classEscape`
    /// escapes only backslash, `]`, `^`, `-` and control characters — and `¡`
    /// (U+00A1) and `ÿ` (U+00FF) are none of those, so the message carries the
    /// characters themselves. Rendering the escape sequence instead is the
    /// obvious mistake, and it makes every one of the twenty-three recorded
    /// error messages wrong.
    Class,
    /// `.` — any single character, from the body of a comment.
    Any,
    /// End of input.
    End,
}

impl Expectation {
    /// The generator's `describeExpectation`, whose output is what gets sorted.
    fn describe(self) -> String {
        match self {
            Self::Literal(s) => format!("\"{}\"", literal_escape(s)),
            Self::Class => "[!-~\u{a1}-\u{ff}]".to_string(),
            Self::Any => "any character".to_string(),
            Self::End => "end of input".to_string(),
        }
    }
}

/// The generator's `literalEscape` / `classEscape`: backslash, quote, then control
/// characters as `\0`, `\t`, `\n`, `\r`, `\x0N` and `\xNN`.
fn literal_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\0' => out.push_str("\\0"),
            '\t' => out.push_str("\\t"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            c if (c as u32) < 0x10 => out.push_str(&format!("\\x0{:X}", c as u32)),
            c if (c as u32) < 0x20 || ((c as u32) >= 0x7f && (c as u32) <= 0x9f) => {
                out.push_str(&format!("\\x{:X}", c as u32));
            }
            c => out.push(c),
        }
    }
    out
}

/// A rule string that did not parse.
///
/// [`Display`](fmt::Display) reproduces `peg$SyntaxError.message` byte for byte,
/// which is what `fixtures/tagger.json` records.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyntaxError {
    /// Everything the parser would have accepted, sorted and deduplicated.
    pub expected: Vec<Expectation>,
    /// The offending character, or `None` at end of input.
    ///
    /// # Deviation
    ///
    /// The generated parser reports `charAt(pos)` — **one UTF-16 code unit** — so for an
    /// astral character it is a lone surrogate. A Rust `char` cannot hold one, so
    /// this crate reports `U+FFFD` instead, matching
    /// [`docs/PARITY.md`](../../../docs/PARITY.md) divergence D2. The fixture
    /// generator applies the same substitution, because no validating JSON
    /// parser can read a lone surrogate back either.
    pub found: Option<char>,
    /// Where the failure is.
    pub location: Location,
}

impl fmt::Display for SyntaxError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // describeExpected: describe, sort, dedup adjacent, join.
        let mut d: Vec<String> = self.expected.iter().map(|e| e.describe()).collect();
        d.sort();
        d.dedup();
        let expected = match d.len() {
            0 => String::new(),
            1 => d[0].clone(),
            2 => format!("{} or {}", d[0], d[1]),
            n => format!("{}, or {}", d[..n - 1].join(", "), d[n - 1]),
        };
        match self.found {
            Some(c) => write!(
                f,
                "Expected {expected} but \"{}\" found.",
                literal_escape(&c.to_string())
            ),
            None => write!(f, "Expected {expected} but end of input found."),
        }
    }
}

impl std::error::Error for SyntaxError {}

/// A parsed rule string: the five-element `literal` array the reference builds.
///
/// Parameters beyond the second are **dropped along with the first two**:
/// `"A B C d e f"` yields `("A", "B", "C", None, None)`, not `("A","B","C","d","e")`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedRule {
    /// Old category. `"*"` here is the wildcard.
    pub category1: String,
    /// New category.
    pub category2: String,
    /// Predicate name.
    pub predicate: String,
    /// First parameter, if the rule supplied exactly one or two.
    pub parameter1: Option<String>,
    /// Second parameter, if the rule supplied exactly two.
    pub parameter2: Option<String>,
}

/// Whether `c` is in the grammar's identifier class `[!-~\xA1-\xFF]`.
///
/// `U+0080`–`U+00A0`, everything from `U+0100` up, and every surrogate half are
/// excluded. Note `/` and `*` **are** included, which is why `"// x y"` parses as
/// three identifiers rather than as a comment.
#[inline]
const fn is_identifier_char(c: char) -> bool {
    matches!(c, '\u{21}'..='\u{7e}' | '\u{a1}'..='\u{ff}')
}

/// Bit flags for the expectation set, so accumulating costs nothing.
mod exp {
    /// `"*"`.
    pub const STAR: u8 = 1 << 0;
    /// `" "`.
    pub const SPACE: u8 = 1 << 1;
    /// `"\t"`.
    pub const TAB: u8 = 1 << 2;
    /// `"//"`.
    pub const SLASHES: u8 = 1 << 3;
    /// The identifier class.
    pub const CLASS: u8 = 1 << 4;
    /// End of input.
    pub const END: u8 = 1 << 5;
    /// `"\r\n"`, `"\n"`, `"\r"` — recorded together, since `EOL` tries all three.
    pub const EOL: u8 = 1 << 6;
    /// `.` inside a comment body, which can only fail at end of input.
    pub const ANY: u8 = 1 << 7;
}

struct Parser<'a> {
    input: &'a str,
    /// Byte cursor.
    at: usize,
    /// UTF-16 cursor, which is what the generated parser reports.
    u16_at: usize,
    max_fail_u16: usize,
    max_fail_at: usize,
    expected: u8,
}

impl<'a> Parser<'a> {
    fn new(input: &'a str) -> Self {
        Self {
            input,
            at: 0,
            u16_at: 0,
            max_fail_u16: 0,
            max_fail_at: 0,
            expected: 0,
        }
    }

    #[inline]
    fn peek(&self) -> Option<char> {
        self.input[self.at..].chars().next()
    }

    #[inline]
    fn bump(&mut self, c: char) {
        self.at += c.len_utf8();
        self.u16_at += c.len_utf16();
    }

    /// `peg$fail`: expectations accumulate only at the furthest failure.
    #[inline]
    fn fail(&mut self, what: u8) {
        if self.u16_at < self.max_fail_u16 {
            return;
        }
        if self.u16_at > self.max_fail_u16 {
            self.max_fail_u16 = self.u16_at;
            self.max_fail_at = self.at;
            self.expected = 0;
        }
        self.expected |= what;
    }

    fn eat_char(&mut self, want: char, what: u8) -> bool {
        if self.peek() == Some(want) {
            self.bump(want);
            true
        } else {
            self.fail(what);
            false
        }
    }

    fn eat_str(&mut self, want: &str, what: u8) -> bool {
        if self.input[self.at..].starts_with(want) {
            for c in want.chars() {
                self.bump(c);
            }
            true
        } else {
            self.fail(what);
            false
        }
    }

    /// `EOL = "\r\n" / "\n" / "\r"`, recording all three on failure.
    fn eat_eol(&mut self) -> bool {
        let rest = &self.input[self.at..];
        let n = if rest.starts_with("\r\n") {
            2
        } else if rest.starts_with('\n') || rest.starts_with('\r') {
            1
        } else {
            self.fail(exp::EOL);
            return false;
        };
        for c in rest.chars().take(n) {
            self.bump(c);
        }
        true
    }

    /// `Comment = "//" (!EOL .)* (EOL / EOI)`.
    ///
    /// The `!EOL` lookahead runs under the generated parser's `silentFails`, so the inner EOL
    /// attempts record nothing — but two things outside it are not silent, and
    /// both show up in real messages when a comment runs to end of input:
    /// the `.` that fails there (`any character`), and the trailing `EOL / EOI`
    /// whose `EOL` branch records `"\r\n"`, `"\n"` and `"\r"` before `EOI`
    /// succeeds. Hence `TFParser.parse("A B //x")` reports seven expectations.
    fn comment(&mut self) -> bool {
        if !self.eat_str("//", exp::SLASHES) {
            return false;
        }
        loop {
            let rest = &self.input[self.at..];
            if rest.is_empty() {
                // `!EOL` succeeds at end of input, then `.` fails there.
                self.fail(exp::ANY);
                break;
            }
            if rest.starts_with('\n') || rest.starts_with('\r') {
                break;
            }
            let c = rest.chars().next().expect("non-empty");
            self.bump(c);
        }
        // (EOL / EOI): one of the two always succeeds here.
        let _ = self.eat_eol();
        true
    }

    /// `S_no_eol = (" " / "\t" / Comment)*` — never fails, always records.
    fn s_no_eol(&mut self) {
        loop {
            if self.eat_char(' ', exp::SPACE) {
                continue;
            }
            if self.eat_char('\t', exp::TAB) {
                continue;
            }
            if self.comment() {
                continue;
            }
            return;
        }
    }

    /// `identifier = [!-~\xA1-\xFF]+ S_no_eol`.
    fn identifier(&mut self) -> Option<String> {
        let start = self.at;
        loop {
            match self.peek() {
                Some(c) if is_identifier_char(c) => self.bump(c),
                _ => {
                    self.fail(exp::CLASS);
                    break;
                }
            }
        }
        if self.at == start {
            return None;
        }
        let text = self.input[start..self.at].to_string();
        self.s_no_eol();
        Some(text)
    }

    /// `wild_card = "*" S_no_eol`, tried before `identifier` in `category1`.
    ///
    /// Because it wins the ordered choice and PEG never revisits it, `"**"` is
    /// wildcard-then-identifier and `"*A B C"` has four fields — the `*` does not
    /// glue to what follows even though `*` is also an identifier character.
    fn wild_card(&mut self) -> Option<String> {
        if self.peek() == Some('*') {
            self.bump('*');
            self.s_no_eol();
            Some("*".to_string())
        } else {
            self.fail(exp::STAR);
            None
        }
    }

    /// Turns the recorded failure into the error the generated parser would have thrown.
    fn error(mut self) -> SyntaxError {
        let mut expected = Vec::new();
        for (bit, e) in [
            (exp::STAR, Expectation::Literal("*")),
            (exp::SPACE, Expectation::Literal(" ")),
            (exp::TAB, Expectation::Literal("\t")),
            (exp::SLASHES, Expectation::Literal("//")),
            (exp::CLASS, Expectation::Class),
            (exp::ANY, Expectation::Any),
            (exp::END, Expectation::End),
        ] {
            if self.expected & bit != 0 {
                expected.push(e);
            }
        }
        if self.expected & exp::EOL != 0 {
            expected.push(Expectation::Literal("\r\n"));
            expected.push(Expectation::Literal("\n"));
            expected.push(Expectation::Literal("\r"));
        }

        let found = self.input[self.max_fail_at..].chars().next();
        // A lone surrogate cannot be represented; see the `found` field's note.
        let found = found.map(|c| if c.len_utf16() == 2 { '\u{fffd}' } else { c });
        let start = self.position_of(self.max_fail_at, self.max_fail_u16);
        let end = match found {
            None => start,
            Some(_) => {
                let c = self.input[self.max_fail_at..]
                    .chars()
                    .next()
                    .expect("found is Some");
                // The generated parser advances exactly one code unit past the offending one.
                let after_bytes = self.max_fail_at + c.len_utf8();
                let after_u16 = self.max_fail_u16 + 1;
                if c == '\n' {
                    // `peg$computePosDetails` increments the line as it *steps
                    // over* the newline, so the position one code unit past it is
                    // already on the next line at column 1 — `line_at` counts
                    // that newline, and adding one more would double-count it.
                    Position {
                        offset: after_u16,
                        line: self.line_at(after_bytes),
                        column: 1,
                    }
                } else {
                    Position {
                        offset: after_u16,
                        line: start.line,
                        column: start.column + 1,
                    }
                }
            }
        };
        self.expected = 0;
        SyntaxError {
            expected,
            found,
            location: Location { start, end },
        }
    }

    fn line_at(&self, byte: usize) -> usize {
        1 + self.input[..byte].matches('\n').count()
    }

    fn position_of(&self, byte: usize, u16_off: usize) -> Position {
        let prefix = &self.input[..byte];
        let line = 1 + prefix.matches('\n').count();
        let col_src = match prefix.rfind('\n') {
            Some(nl) => &prefix[nl + 1..],
            None => prefix,
        };
        let column = 1 + col_src.chars().map(char::len_utf16).sum::<usize>();
        debug_assert_eq!(
            u16_off,
            prefix.chars().map(char::len_utf16).sum::<usize>(),
            "utf16 cursor drifted"
        );
        Position {
            offset: u16_off,
            line,
            column,
        }
    }
}

/// Parses one rule string.
///
/// # Errors
///
/// Returns the generated parser's `SyntaxError` for any input the grammar does not accept in
/// full: leading whitespace, an embedded newline, a character outside
/// `[!-~\xA1-\xFF]`, or fewer than three fields.
pub fn parse(input: &str) -> Result<ParsedRule, SyntaxError> {
    let mut p = Parser::new(input);

    // category1 = wild_card / identifier
    let Some(c1) = p.wild_card().or_else(|| p.identifier()) else {
        return Err(p.error());
    };
    let Some(c2) = p.identifier() else {
        return Err(p.error());
    };
    let Some(pred) = p.identifier() else {
        return Err(p.error());
    };

    let mut pars: Vec<String> = Vec::new();
    while let Some(v) = p.identifier() {
        pars.push(v);
    }

    if p.at != input.len() {
        // peg$fail(peg$endExpectation()) at the current position, which is also
        // the furthest failure — the parameter loop just failed there.
        p.fail(exp::END);
        return Err(p.error());
    }

    // The action keeps parameters only when there are exactly one or two of
    // them; three or more drops BOTH.
    let (parameter1, parameter2) = match pars.len() {
        1 => (Some(pars.remove(0)), None),
        2 => {
            let b = pars.pop();
            (pars.pop(), b)
        }
        _ => (None, None),
    };

    Ok(ParsedRule {
        category1: c1,
        category2: c2,
        predicate: pred,
        parameter1,
        parameter2,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ok(s: &str) -> ParsedRule {
        parse(s).unwrap_or_else(|e| panic!("{s:?} should parse: {e}"))
    }

    fn err(s: &str) -> String {
        parse(s).expect_err(s).to_string()
    }

    #[test]
    fn field_counts() {
        assert_eq!(ok("A B C").parameter1, None);
        assert_eq!(ok("A B C d").parameter1.as_deref(), Some("d"));
        assert_eq!(ok("A B C d e").parameter2.as_deref(), Some("e"));
        // Three or more parameters drop BOTH, which a splitter would not do.
        assert_eq!(ok("A B C d e f").parameter1, None);
        assert_eq!(ok("A B C d e f g").parameter1, None);
    }

    #[test]
    fn slashes_are_identifier_characters() {
        let r = ok("// only comment");
        assert_eq!(r.category1, "//");
        assert_eq!(r.category2, "only");
        assert_eq!(r.predicate, "comment");
        assert_eq!(ok("A B C//x").predicate, "C//x");
        assert_eq!(ok("A/ B C").category1, "A/");
    }

    #[test]
    fn wildcard_is_exactly_one_character() {
        let r = ok("*A B C");
        assert_eq!(r.category1, "*");
        assert_eq!(r.category2, "A");
        assert_eq!(r.parameter1.as_deref(), Some("C"));
        assert_eq!(ok("A B *").predicate, "*");
        assert_eq!(ok("* * *").category2, "*");
        assert!(parse("**").is_err());
    }

    #[test]
    fn comments_are_whitespace_after_a_space() {
        assert_eq!(ok("A B C // comment").predicate, "C");
        assert_eq!(ok("A B C //").predicate, "C");
        // A newline inside a comment is consumed, so the rule continues.
        assert_eq!(ok("A B C //x\nD").parameter1.as_deref(), Some("D"));
        assert_eq!(ok("A B C  \t //c\r\n D").parameter1.as_deref(), Some("D"));
    }

    #[test]
    fn a_comment_running_to_end_of_input_widens_the_expectation_set() {
        // The `.` inside `(!EOL .)*` and the trailing `EOL / EOI` both record at
        // end of input, so seven expectations reach the message — a detail a
        // reimplementation that treated comments as plain whitespace loses.
        assert_eq!(
            err("A B //x"),
            "Expected \" \", \"//\", \"\\n\", \"\\r\", \"\\r\\n\", \"\\t\", \
             [!-~¡-ÿ], or any character but end of input found."
        );
        // A comment terminated by a real EOL records none of them.
        assert_eq!(
            err("A B //x\r"),
            "Expected \" \", \"//\", \"\\t\", or [!-~¡-ÿ] but end of input found."
        );
    }

    #[test]
    fn leading_whitespace_and_newlines_are_rejected() {
        assert_eq!(err(" A B C"), "Expected \"*\" or [!-~¡-ÿ] but \" \" found.");
        assert_eq!(
            err("\tA B C"),
            "Expected \"*\" or [!-~¡-ÿ] but \"\\t\" found."
        );
        assert_eq!(
            err("A B C\n"),
            "Expected \" \", \"//\", \"\\t\", [!-~¡-ÿ], or end of input but \"\\n\" found."
        );
    }

    #[test]
    fn character_class_boundaries() {
        assert_eq!(ok("Aÿ B C").category1, "Aÿ"); // U+00FF is the top of the class
        assert_eq!(ok("Aé B C").category1, "Aé");
        // U+00A1 is the bottom of the class, so "A¡B" is ONE identifier and the
        // rule is left a field short — the failure is arity, not the character.
        assert_eq!(
            err("A¡B C"),
            "Expected \" \", \"//\", \"\\t\", or [!-~¡-ÿ] but end of input found."
        );
        assert!(parse("AĀ B C").is_err()); // U+0100 is out
        assert!(parse("A B C\u{7f}").is_err()); // DEL is out
        assert!(parse("A B C\u{a0}").is_err()); // NBSP is out
        assert!(parse("A😀 B C").is_err()); // astral: a surrogate half is out
    }

    #[test]
    fn empty_and_short_inputs() {
        assert_eq!(
            err(""),
            "Expected \"*\" or [!-~¡-ÿ] but end of input found."
        );
        assert_eq!(
            err("A"),
            "Expected \" \", \"//\", \"\\t\", or [!-~¡-ÿ] but end of input found."
        );
        assert_eq!(
            err("A B"),
            "Expected \" \", \"//\", \"\\t\", or [!-~¡-ÿ] but end of input found."
        );
    }

    #[test]
    fn locations_count_utf16_units_and_lf_lines() {
        let e = parse("A B C\n").expect_err("newline");
        assert_eq!(e.location.start.offset, 5);
        assert_eq!(e.location.start.line, 1);
        assert_eq!(e.location.start.column, 6);
        assert_eq!(e.location.end.line, 2);
        assert_eq!(e.location.end.column, 1);

        // \r does not start a new line.
        let e = parse("A B C\r\n").expect_err("cr");
        assert_eq!(e.location.end.line, 1);
        assert_eq!(e.location.end.column, 7);
    }

    #[test]
    fn astral_found_is_replaced() {
        let e = parse("A😀 B C").expect_err("astral");
        assert_eq!(e.found, Some('\u{fffd}'));
        assert_eq!(e.location.start.offset, 1);
    }

    #[test]
    fn real_rule_strings() {
        assert_eq!(ok("VBD NN PREV-TAG DT").parameter1.as_deref(), Some("DT"));
        let r = ok("Adj(adv,stell,onverv) Adj(attr,stell,onverv) WDPREVTAG N(soort,mv,neut) nodig");
        assert_eq!(r.parameter1.as_deref(), Some("N(soort,mv,neut)"));
        assert_eq!(r.parameter2.as_deref(), Some("nodig"));
    }
}
