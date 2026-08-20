use std::borrow::Cow;
use std::fmt;

use regex::Regex;

/// A single rewrite rule in an inflector's ordered rule list.
///
/// A rule pairs a pattern with an action. [`Rule::new`] rewrites the first match
/// using a replacement template; [`Rule::keep`] matches and leaves the token
/// alone, which is how an ordered list expresses "stop here" — `dresses` must
/// not become `dresseses`, so a guard for the already-plural shape precedes the
/// sibilant rule.
///
/// # Pattern syntax
///
/// Patterns are ordinary [`regex`](https://docs.rs/regex) crate patterns, with
/// that crate's semantics in full: leftmost-first alternation, `.` excluding
/// `\n` unless `(?s)` is set, `$` matching only at the end of the token, and
/// inline flags such as `(?i)`. Case-insensitive matching therefore uses
/// **Unicode simple case folding** (UTS #18 §1.5, from the UCD `CaseFolding.txt`
/// `C` and `S` mappings), so `(?i)s$` matches `U+017F ſ`. Every built-in rule
/// that must ignore case spells `(?i)` in its own pattern rather than inheriting
/// a flag.
///
/// Lookaround and backreferences do not exist in the `regex` crate and so cannot
/// be used. Where a rule needs "except these words", the exception is a separate
/// rule placed *before* it — see the English `-man` pair, where
/// `human`/`talisman`/`shaman` take the regular plural through an earlier rule
/// and everything else reaches `man$ → men`.
///
/// # Replacement syntax
///
/// A replacement is literal text plus **brace-delimited** group references:
/// `${0}` for the whole match, `${1}`, `${2}`, … for numbered groups, and
/// `${name}` for a named group. `$$` is a literal `$`. A `$` followed by
/// anything else is a literal `$`.
///
/// A bare `$1` is **rejected**, not accepted: in the `regex` crate `"$1s"` names
/// the group `1s`, so a template written the way it would be in `sed` or in
/// JavaScript would silently expand to nothing and delete the suffix. Requiring
/// the braces turns that silent wrong answer into a [`RuleError`] at
/// construction. Group names that the pattern does not declare are rejected for
/// the same reason.
///
/// # Empty results
///
/// A rule that matches but would rewrite a non-empty token to the empty string
/// **declines**: [`Rule::apply`] returns `None` and the inflector moves on to the
/// next rule. An inflected form of a word is a word, and a rule that erases its
/// input has not produced one. This is why the English verb rule `(?i)e?s$ → ""`
/// leaves the one-character token `"s"` alone instead of returning `""`.
///
/// ```
/// use verbora_inflectors::Rule;
///
/// let rule = Rule::new("(?i)(x|ch|ss|sh|s|z)$", "${1}es").unwrap();
/// assert_eq!(rule.apply("church").as_deref(), Some("churches"));
/// assert_eq!(rule.apply("cat"), None);
///
/// let guard = Rule::keep("(?i)(ses|xes|zes|ches|shes)$").unwrap();
/// assert_eq!(guard.apply("dresses").as_deref(), Some("dresses"));
///
/// // A bare group reference is refused rather than silently expanded to "".
/// assert!(Rule::new("(a)", "$1s").is_err());
/// ```
pub struct Rule {
    re: Regex,
    action: Action,
}

enum Action {
    /// Rewrite the first match with this template.
    Replace {
        template: String,
        /// Whether the template contains any `$`, and so needs capture slots.
        expands: bool,
    },
    /// Match and leave the token as it is.
    Keep,
}

/// What a rule did to a token, distinguishing "unchanged" from "rewritten".
pub(crate) enum Applied {
    /// The rule matched and the token is its own result.
    Unchanged,
    /// The rule matched and produced a different form.
    Form(String),
}

/// Why a [`Rule`] could not be built.
///
/// Every variant is a construction-time refusal. Once a `Rule` exists,
/// [`Rule::apply`] is total: it cannot fail, cannot panic and cannot expand a
/// group the pattern does not have.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum RuleError {
    /// The pattern is not valid `regex`-crate syntax.
    ///
    /// `message` is the `regex` crate's own diagnostic, captured as text so this
    /// crate's public API does not depend on that crate's error type.
    Pattern {
        /// The pattern as supplied.
        pattern: String,
        /// The compiler's diagnostic.
        message: String,
    },
    /// The replacement contains a `$` group reference without braces, such as
    /// `$1`.
    ///
    /// Write `${1}` instead. See the [`Rule`] documentation for why an unbraced
    /// reference is refused rather than interpreted.
    BareGroupReference {
        /// Byte offset of the `$` inside the replacement.
        offset: usize,
    },
    /// The replacement contains a `${` with no closing `}`.
    UnterminatedGroupReference {
        /// Byte offset of the `$` inside the replacement.
        offset: usize,
    },
    /// The replacement refers to a group the pattern does not declare.
    UnknownGroup {
        /// The text between the braces.
        name: String,
    },
}

impl fmt::Display for RuleError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Pattern { pattern, message } => {
                write!(f, "pattern `{pattern}` did not compile: {message}")
            }
            Self::BareGroupReference { offset } => write!(
                f,
                "replacement byte {offset}: a group reference must be brace-delimited, \
                 as in `${{1}}`; a bare `$1` would name the group `1s` in `$1s`"
            ),
            Self::UnterminatedGroupReference { offset } => {
                write!(f, "replacement byte {offset}: `${{` has no closing `}}`")
            }
            Self::UnknownGroup { name } => {
                write!(
                    f,
                    "replacement refers to group `{name}`, which the pattern does not declare"
                )
            }
        }
    }
}

impl std::error::Error for RuleError {}

impl Rule {
    /// Builds a rule that rewrites the first match of `pattern` with
    /// `replacement`.
    ///
    /// # Errors
    ///
    /// [`RuleError::Pattern`] when the pattern does not compile, and the three
    /// replacement variants when the template is malformed or names a group the
    /// pattern does not declare. See the [type documentation](Rule) for the
    /// replacement syntax.
    pub fn new(pattern: &str, replacement: impl Into<String>) -> Result<Self, RuleError> {
        let re = compile(pattern)?;
        let template = replacement.into();
        validate_template(&template, &re)?;
        let expands = template.contains('$');
        Ok(Self {
            re,
            action: Action::Replace { template, expands },
        })
    }

    /// Builds a rule that matches `pattern` and leaves the token unchanged.
    ///
    /// Guards are how an ordered rule list says "this shape is already the form
    /// we were asked for, stop scanning".
    ///
    /// # Errors
    ///
    /// [`RuleError::Pattern`] when the pattern does not compile.
    pub fn keep(pattern: &str) -> Result<Self, RuleError> {
        Ok(Self {
            re: compile(pattern)?,
            action: Action::Keep,
        })
    }

    /// The pattern this rule matches, as written.
    #[must_use]
    pub fn pattern(&self) -> &str {
        self.re.as_str()
    }

    /// Applies the rule to `token`.
    ///
    /// Returns `None` when the pattern does not match, and also when it matches
    /// but the rewrite would empty a non-empty token — see
    /// [Empty results](Rule#empty-results). A [`Rule::keep`] rule that matches
    /// borrows `token` rather than copying it.
    #[must_use]
    pub fn apply<'t>(&self, token: &'t str) -> Option<Cow<'t, str>> {
        match self.apply_inner(token)? {
            Applied::Unchanged => Some(Cow::Borrowed(token)),
            Applied::Form(form) => Some(Cow::Owned(form)),
        }
    }

    /// [`Rule::apply`] without collapsing "unchanged" into "a copy of the
    /// token", which the engine needs in order to skip case restoration for a
    /// token that is already its own answer.
    pub(crate) fn apply_inner(&self, token: &str) -> Option<Applied> {
        let template = match &self.action {
            Action::Keep => {
                return self.re.is_match(token).then_some(Applied::Unchanged);
            }
            Action::Replace { template, expands } => {
                if !expands {
                    let m = self.re.find(token)?;
                    let mut out = String::with_capacity(token.len() + template.len());
                    out.push_str(token.get(..m.start())?);
                    out.push_str(template);
                    out.push_str(token.get(m.end()..)?);
                    return finish(out, token);
                }
                template
            }
        };

        // `Regex::captures` allocates capture slots on every call, and an
        // inflector runs up to eighteen patterns before one fires; testing first
        // keeps the seventeen misses allocation-free.
        if !self.re.is_match(token) {
            return None;
        }
        let caps = self.re.captures(token)?;
        let whole = caps.get(0)?;
        let mut out = String::with_capacity(token.len() + template.len());
        out.push_str(token.get(..whole.start())?);
        caps.expand(template, &mut out);
        out.push_str(token.get(whole.end()..)?);
        finish(out, token)
    }
}

/// Rejects a rewrite that would empty a non-empty token, and reports an
/// unchanged result as such.
fn finish(out: String, token: &str) -> Option<Applied> {
    if out.is_empty() && !token.is_empty() {
        return None;
    }
    if out == token {
        return Some(Applied::Unchanged);
    }
    Some(Applied::Form(out))
}

fn compile(pattern: &str) -> Result<Regex, RuleError> {
    Regex::new(pattern).map_err(|e| RuleError::Pattern {
        pattern: pattern.to_owned(),
        message: e.to_string(),
    })
}

/// Enforces the brace-delimited replacement syntax against a compiled pattern.
fn validate_template(template: &str, re: &Regex) -> Result<(), RuleError> {
    let bytes = template.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] != b'$' {
            i += 1;
            continue;
        }
        match bytes.get(i + 1) {
            // `$$` is a literal dollar.
            Some(b'$') => i += 2,
            Some(b'{') => {
                let rest = template.get(i + 2..).unwrap_or_default();
                let end = rest
                    .find('}')
                    .ok_or(RuleError::UnterminatedGroupReference { offset: i })?;
                let name = &rest[..end];
                if !group_exists(name, re) {
                    return Err(RuleError::UnknownGroup {
                        name: name.to_owned(),
                    });
                }
                i += 2 + end + 1;
            }
            Some(c) if c.is_ascii_alphanumeric() || *c == b'_' => {
                return Err(RuleError::BareGroupReference { offset: i });
            }
            // A `$` before anything else, or at the end, is a literal dollar.
            _ => i += 1,
        }
    }
    Ok(())
}

fn group_exists(name: &str, re: &Regex) -> bool {
    if let Ok(index) = name.parse::<usize>() {
        return index < re.captures_len();
    }
    re.capture_names().flatten().any(|n| n == name)
}

impl fmt::Debug for Rule {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut s = f.debug_struct("Rule");
        s.field("pattern", &self.re.as_str());
        match &self.action {
            Action::Replace { template, .. } => s.field("replacement", template),
            Action::Keep => s.field("action", &"keep"),
        };
        s.finish()
    }
}

#[cfg(test)]
mod tests {
    use super::{Rule, RuleError};

    #[test]
    fn a_literal_template_rewrites_the_first_match_only() {
        let rule = Rule::new("a", "X").unwrap();
        // Non-global by definition: `replace_all` would give "XX".
        assert_eq!(rule.apply("aa").as_deref(), Some("Xa"));
        assert_eq!(rule.apply("bb"), None);
    }

    #[test]
    fn braced_group_references_expand() {
        let rule = Rule::new("(?i)(x|ch|ss|sh|s|z)$", "${1}es").unwrap();
        assert_eq!(rule.apply("church").as_deref(), Some("churches"));
        assert_eq!(rule.apply("box").as_deref(), Some("boxes"));
        assert_eq!(rule.apply("CHURCH").as_deref(), Some("CHURCHes"));

        let whole = Rule::new("(?i)man$", "${0}kind").unwrap();
        assert_eq!(whole.apply("workman").as_deref(), Some("workmankind"));

        let named = Rule::new("(?i)(?P<stem>.+)ing$", "${stem}ed").unwrap();
        assert_eq!(named.apply("walking").as_deref(), Some("walked"));
    }

    #[test]
    fn a_bare_group_reference_is_refused() {
        // The whole point: `$1s` names the group `1s` in the regex crate and
        // would expand to "", deleting the suffix without any diagnostic.
        assert_eq!(
            Rule::new("(a)", "$1s").unwrap_err(),
            RuleError::BareGroupReference { offset: 0 }
        );
        assert_eq!(
            Rule::new("(a)", "x$2y").unwrap_err(),
            RuleError::BareGroupReference { offset: 1 }
        );
        // Braced is accepted.
        assert!(Rule::new("(a)", "${1}s").is_ok());
        // A dollar that is not a group reference stays literal.
        assert!(Rule::new("(a)", "costs $").is_ok());
        assert!(Rule::new("(a)", "$$5").is_ok());
        assert!(Rule::new("(a)", "$ 5").is_ok());
    }

    #[test]
    fn a_group_the_pattern_lacks_is_refused() {
        assert_eq!(
            Rule::new("(a)", "${2}").unwrap_err(),
            RuleError::UnknownGroup {
                name: "2".to_owned()
            }
        );
        assert_eq!(
            Rule::new("(a)", "${nope}").unwrap_err(),
            RuleError::UnknownGroup {
                name: "nope".to_owned()
            }
        );
        assert_eq!(
            Rule::new("(a)", "${1").unwrap_err(),
            RuleError::UnterminatedGroupReference { offset: 0 }
        );
        // Group 0 is always available.
        assert!(Rule::new("a", "${0}!").is_ok());
    }

    #[test]
    fn an_invalid_pattern_is_reported_with_its_text() {
        let err = Rule::new("(unclosed", "x").unwrap_err();
        match err {
            RuleError::Pattern { pattern, message } => {
                assert_eq!(pattern, "(unclosed");
                assert!(!message.is_empty());
            }
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn a_rewrite_that_would_empty_the_token_declines() {
        let rule = Rule::new("(?i)e?s$", "").unwrap();
        assert_eq!(rule.apply("runs").as_deref(), Some("run"));
        // "s" would become "", so the rule declines instead.
        assert_eq!(rule.apply("s"), None);
        assert_eq!(rule.apply("es"), None);
        // The empty token itself is not special-cased here; the inflectors
        // return it before any rule runs.
        assert_eq!(rule.apply(""), None);
    }

    #[test]
    fn keep_rules_borrow_the_token() {
        let guard = Rule::keep("(?i)(ses|xes|zes|ches|shes)$").unwrap();
        let out = guard.apply("dresses").expect("guard matches");
        assert!(matches!(out, std::borrow::Cow::Borrowed("dresses")));
        assert_eq!(guard.apply("dress"), None);
    }

    #[test]
    fn case_insensitivity_is_unicode_simple_folding() {
        // The documented basis: `(?i)` folds U+017F to `s`.
        let rule = Rule::new("(?i)s$", "${0}es").unwrap();
        assert_eq!(rule.apply("bus").as_deref(), Some("buses"));
        assert_eq!(rule.apply("bu\u{17f}").as_deref(), Some("bu\u{17f}es"));
        assert_eq!(rule.apply("BUS").as_deref(), Some("BUSes"));
    }

    #[test]
    fn multibyte_tokens_are_spliced_on_character_boundaries() {
        let rule = Rule::new("(?i)man$", "men").unwrap();
        assert_eq!(rule.apply("日本man").as_deref(), Some("日本men"));
        assert_eq!(rule.apply("👍man").as_deref(), Some("👍men"));
        assert_eq!(rule.apply("mañ"), None);

        let ja = Rule::new("^(.+)たち$", "${1}").unwrap();
        assert_eq!(ja.apply("人たち").as_deref(), Some("人"));
    }

    #[test]
    fn end_anchor_appends_without_touching_line_terminators() {
        let rule = Rule::new("$", "s").unwrap();
        assert_eq!(rule.apply("hacker").as_deref(), Some("hackers"));
        // `$` matches only at the end of the token, so an embedded newline is
        // carried through untouched rather than cutting the match short.
        assert_eq!(rule.apply("a\nb").as_deref(), Some("a\nbs"));
        assert_eq!(rule.apply("a\u{2028}b").as_deref(), Some("a\u{2028}bs"));
    }

    #[test]
    fn debug_shows_the_pattern_and_the_action() {
        let replace = format!("{:?}", Rule::new("(?i)y$", "ies").unwrap());
        assert!(replace.contains("(?i)y$"), "{replace}");
        assert!(replace.contains("ies"), "{replace}");
        let keep = format!("{:?}", Rule::keep("(?i)ss$").unwrap());
        assert!(keep.contains("keep"), "{keep}");
    }
}
