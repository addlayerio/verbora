//! `String.prototype.replace` substitution templates.
//!
//! The inflectors' replacements are the reference templates (`'$1es'`,
//! `'$1aïeul'`, `'ses'`). They are *not* `regex`-crate templates, and the two
//! disagree in a way that fails silently:
//!
//! ```text
//! template   the reference             Rust `Captures::expand`
//! "$1s"      group 1, then "s"         group named "1s"  → ""
//! "$0"       the literal "$0"          the whole match
//! "$12"      group 12, else group 1    group named "12"  → ""
//!            followed by "2"
//! ```
//!
//! Every one of the reference's ~50 replacements is of the `$1` + letters shape,
//! so handing them to `expand` would erase the suffix from all of them and turn
//! `"child"` into `""`. Writing `${1}s` instead would fix the built-ins but not
//! a user-supplied template, which arrives in the reference syntax by definition.
//! Implementing the reference rules once is both the faithful and the smaller
//! option.

use regex::{Captures, Match, Regex};

/// What a template is expanded against: a bare match span, or full captures.
///
/// The distinction is a performance one, not a semantic one. `Regex::captures`
/// allocates a slot vector per call; `Regex::find` does not. Roughly half the
/// reference's replacements never mention a group (`'ies'`, `'sses'`, `'man'`,
/// `'ses'`), and those can be expanded from the span alone — which matters
/// because a rule set runs up to twelve patterns per word.
pub(crate) enum MatchRef<'c, 'h> {
    /// A span only; group references are unavailable and stay literal.
    Whole(Match<'h>),
    /// Full captures.
    Captured(&'c Captures<'h>),
}

impl<'h> MatchRef<'_, 'h> {
    /// Byte offset where the match starts.
    pub(crate) fn start(&self) -> usize {
        match self {
            Self::Whole(m) => m.start(),
            Self::Captured(c) => c.get(0).expect("group 0 always participates").start(),
        }
    }

    /// Byte offset just past the match.
    pub(crate) fn end(&self) -> usize {
        match self {
            Self::Whole(m) => m.end(),
            Self::Captured(c) => c.get(0).expect("group 0 always participates").end(),
        }
    }

    /// The matched text, i.e. `$&`.
    fn text(&self) -> &'h str {
        match self {
            Self::Whole(m) => m.as_str(),
            Self::Captured(c) => c.get(0).expect("group 0 always participates").as_str(),
        }
    }

    /// The text of a numbered group, or `None` if it is absent or did not
    /// participate.
    pub(crate) fn group(&self, index: usize) -> Option<&'h str> {
        match self {
            Self::Whole(_) => None,
            Self::Captured(c) => c.get(index).map(|m| m.as_str()),
        }
    }

    /// The text of a named group.
    fn named(&self, name: &str) -> Option<&'h str> {
        match self {
            Self::Whole(_) => None,
            Self::Captured(c) => c.name(name).map(|m| m.as_str()),
        }
    }

    /// How many numbered groups the pattern declares.
    fn group_count(&self) -> usize {
        match self {
            // A `Whole` match is only ever used for templates that reference no
            // groups, so reporting zero cannot change an expansion.
            Self::Whole(_) => 0,
            Self::Captured(c) => c.len() - 1,
        }
    }
}

/// Whether a template reads a capture group, and therefore needs
/// [`MatchRef::Captured`].
///
/// Deliberately conservative: any `$` followed by a digit or `<` counts, even
/// where the group turns out not to exist, so a template can never be expanded
/// with fewer groups available than it asks for.
pub(crate) fn template_reads_groups(template: &str) -> bool {
    let bytes = template.as_bytes();
    let mut i = 0;
    while let Some(offset) = template[i..].find('$') {
        let at = i + offset;
        match bytes.get(at + 1) {
            // `$$` escapes the dollar, so skip both.
            Some(b'$') => i = at + 2,
            Some(b'<') => return true,
            Some(d) if d.is_ascii_digit() => return true,
            _ => i = at + 1,
        }
        if i > template.len() {
            break;
        }
    }
    false
}

/// Expands a reference replacement template against one match.
///
/// `haystack` is the full subject string, needed for `` $` `` and `$'`.
///
/// Recognised forms: `$$` (a literal `$`), `$&` (the match), `` $` `` (the text
/// before it), `$'` (the text after it), `$n`/`$nn` (a capture group), and
/// `$<name>` (a named group, but only when the pattern actually declares one —
/// otherwise the reference treats `$<` literally). A group that did not participate
/// expands to the empty string; an out-of-range `$n` stays literal.
pub(crate) fn expand(
    template: &str,
    re: &Regex,
    matched: &MatchRef<'_, '_>,
    haystack: &str,
    out: &mut String,
) {
    let group_count = matched.group_count();
    let mut rest = template;

    while let Some(pos) = rest.find('$') {
        out.push_str(&rest[..pos]);
        let after = &rest[pos + 1..];
        let bytes = after.as_bytes();

        rest = match bytes.first() {
            // A trailing `$` is a literal.
            None => {
                out.push('$');
                ""
            }
            Some(b'$') => {
                out.push('$');
                &after[1..]
            }
            Some(b'&') => {
                out.push_str(matched.text());
                &after[1..]
            }
            Some(b'`') => {
                out.push_str(&haystack[..matched.start()]);
                &after[1..]
            }
            Some(b'\'') => {
                out.push_str(&haystack[matched.end()..]);
                &after[1..]
            }
            Some(b'<') => expand_named(after, re, matched, out),
            Some(d) if d.is_ascii_digit() => expand_numbered(after, matched, group_count, out),
            // `$` followed by anything else is a literal `$`.
            _ => {
                out.push('$');
                after
            }
        };
    }

    out.push_str(rest);
}

/// Handles `$n` and `$nn`, returning the unconsumed remainder of the template.
///
/// The reference prefers the two-digit reading, falling back to one digit, and
/// leaves the text alone when neither names an existing group — which is why
/// `$0` is literal and `$12` means "group 1 then the character 2" for a pattern
/// with fewer than 12 groups.
fn expand_numbered<'t>(
    after: &'t str,
    matched: &MatchRef<'_, '_>,
    group_count: usize,
    out: &mut String,
) -> &'t str {
    let bytes = after.as_bytes();
    let digit = |b: u8| usize::from(b - b'0');

    if let Some(&second) = bytes.get(1).filter(|b| b.is_ascii_digit()) {
        let two = digit(bytes[0]) * 10 + digit(second);
        if (1..=group_count).contains(&two) {
            push_group(matched, two, out);
            return &after[2..];
        }
    }
    let one = digit(bytes[0]);
    if (1..=group_count).contains(&one) {
        push_group(matched, one, out);
        return &after[1..];
    }
    out.push('$');
    after
}

/// Handles `$<name>`.
///
/// When the pattern declares no named groups, the reference does not treat `$<` as
/// a substitution at all, so the whole sequence stays literal.
fn expand_named<'t>(
    after: &'t str,
    re: &Regex,
    matched: &MatchRef<'_, '_>,
    out: &mut String,
) -> &'t str {
    let has_named = re.capture_names().any(|n| n.is_some());
    let Some(end) = after.find('>') else {
        out.push('$');
        return after;
    };
    if !has_named {
        out.push('$');
        return after;
    }
    let name = &after[1..end];
    if let Some(text) = matched.named(name) {
        out.push_str(text);
    }
    &after[end + 1..]
}

fn push_group(matched: &MatchRef<'_, '_>, index: usize, out: &mut String) {
    if let Some(text) = matched.group(index) {
        out.push_str(text);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn apply(pattern: &str, template: &str, subject: &str) -> String {
        let re = Regex::new(pattern).unwrap();
        let caps = re.captures(subject).expect("pattern should match");
        let matched = MatchRef::Captured(&caps);
        let mut out = String::from(&subject[..matched.start()]);
        expand(template, &re, &matched, subject, &mut out);
        out.push_str(&subject[matched.end()..]);
        out
    }

    #[test]
    fn digit_followed_by_letters_is_a_group_then_literal_text() {
        assert_eq!(apply("(child)", "$1ren", "child"), "children");
        assert_eq!(apply("(x|ch|ss|sh|s|z)$", "$1es", "church"), "churches");
    }

    #[test]
    fn dollar_zero_and_out_of_range_groups_stay_literal() {
        assert_eq!(apply("(a)", "$0", "a"), "$0");
        assert_eq!(apply("(a)", "$2", "a"), "$2");
        // Two-digit reading fails (only one group), so `$1` wins and `2` is text.
        assert_eq!(apply("(a)", "$12", "a"), "a2");
    }

    #[test]
    fn non_participating_group_expands_to_nothing() {
        assert_eq!(apply("^(bis)?aïeux$", "$1aïeul", "aïeux"), "aïeul");
        assert_eq!(apply("^(bis)?aïeux$", "$1aïeul", "bisaïeux"), "bisaïeul");
    }

    #[test]
    fn match_relative_forms() {
        assert_eq!(apply("b", "[$&]", "abc"), "a[b]c");
        assert_eq!(apply("b", "[$`]", "abc"), "a[a]c");
        assert_eq!(apply("b", "[$']", "abc"), "a[c]c");
        assert_eq!(apply("b", "$$", "abc"), "a$c");
    }

    #[test]
    fn lone_dollar_and_unknown_sequences_stay_literal() {
        assert_eq!(apply("b", "$", "abc"), "a$c");
        assert_eq!(apply("b", "$z", "abc"), "a$zc");
        // No named groups declared, so `$<x>` is literal.
        assert_eq!(apply("b", "$<x>", "abc"), "a$<x>c");
    }

    #[test]
    fn group_detection_is_conservative() {
        assert!(template_reads_groups("$1s"));
        assert!(template_reads_groups("x$12y"));
        assert!(template_reads_groups("$<name>"));
        // `$$` escapes the dollar, so the digit after it is literal text.
        assert!(!template_reads_groups("$$1"));
        assert!(!template_reads_groups("ies"));
        assert!(!template_reads_groups(""));
        assert!(!template_reads_groups("$&$`$'$$"));
        assert!(!template_reads_groups("costs $"));
    }

    #[test]
    fn a_span_only_match_leaves_group_references_literal() {
        // Only reachable for templates that reference no groups, but the
        // fallback must still be well defined rather than panicking.
        let re = Regex::new("(a)b").unwrap();
        let m = re.find("ab").unwrap();
        let mut out = String::new();
        expand("[$1|$&]", &re, &MatchRef::Whole(m), "ab", &mut out);
        assert_eq!(out, "[$1|ab]");
    }

    #[test]
    fn named_groups_are_supported_when_declared() {
        assert_eq!(apply("(?P<stem>ab)c", "$<stem>!", "abc"), "ab!");
        assert_eq!(apply("(?P<stem>ab)c", "$<nope>!", "abc"), "!");
    }
}
