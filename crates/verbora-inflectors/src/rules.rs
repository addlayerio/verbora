//! Rewrite rules: the `[pattern, replacement]` pairs `izeRegExps` walks.
//!
//! ```text
//! for (i = 0; i < forms.length; i++) {
//!   form = forms[i]
//!   if (token.match(form[0])) { return token.replace(form[0], form[1]) }
//! }
//! return false
//! ```
//!
//! Two properties of that loop are load-bearing:
//!
//! * **First match wins**, in array order. Several rules exist only to *block*
//!   a later one by rewriting a word to itself — `/sses$/i → 'sses'`,
//!   `/ss$/i → 'ss'`, `/ed$/i → 'ed'`, `/(s|x)$/i → '$1'`. Dropping them as
//!   no-ops changes `dresses`, `dress`, `watched` and `os`.
//! * **The patterns are not global**, so `String.replace` rewrites only the
//!   first match. Rust's `Regex::replace_all` is the wrong default here;
//!   `replace` — or, as below, an explicit splice — is the faithful one.

use std::borrow::Cow;

use regex::Regex;

use crate::data::{RawReplacement, RawRule};
use crate::pattern::{self, PatternError};
use crate::replace::{self, MatchRef, template_reads_groups};

/// The one pattern in the reference that the `regex` crate cannot express.
///
/// `/^(?!talis|.*hu)(.*)man$/i` uses a negative lookahead. It is matched by
/// source text so that [`crate::data`] can stay a verbatim transcription.
pub(crate) const MAN_PLURAL_SOURCE: &str = "^(?!talis|.*hu)(.*)man$";

/// One inflection rule.
///
/// Built-in rules are compiled from the reference's own tables. Callers build
/// their own with [`Rule::from_pattern`] or [`Rule::new`] and register them through
/// [`add_plural`](crate::SingularPluralInflector::add_plural) or
/// [`add_singular`](crate::SingularPluralInflector::add_singular).
pub struct Rule {
    kind: Kind,
}

enum Kind {
    /// A translated the reference pattern plus its replacement.
    Pattern {
        re: Regex,
        replacement: Replacement,
        /// Whether the replacement actually reads a capture group.
        ///
        /// `Regex::captures` allocates a slot vector on every call, and a rule
        /// set runs up to twelve patterns before one fires — so paying for
        /// capture slots on rules that either do not match or do not need groups
        /// is eleven wasted allocations per word. Rules whose template is a
        /// plain string or uses only `$&`/`` $` ``/`$'`/`$$` take the
        /// allocation-free [`Regex::find`] path instead.
        needs_captures: bool,
    },
    /// The hand-written equivalent of `/^(?!talis|.*hu)(.*)man$/i → '$1men'`.
    ManPlural,
}

enum Replacement {
    /// A reference substitution template.
    Template(Cow<'static, str>),
    /// The three Japanese rules whose replacement is a function.
    KeepIfListed {
        exceptions: &'static [&'static str],
        suffix: &'static str,
    },
}

impl std::fmt::Debug for Rule {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.kind {
            Kind::Pattern {
                re, replacement, ..
            } => f
                .debug_struct("Rule")
                .field("pattern", &re.as_str())
                .field(
                    "replacement",
                    match replacement {
                        Replacement::Template(t) => t as &dyn std::fmt::Debug,
                        Replacement::KeepIfListed { suffix, .. } => suffix,
                    },
                )
                .finish(),
            Kind::ManPlural => f.write_str("Rule(/^(?!talis|.*hu)(.*)man$/i -> \"$1men\")"),
        }
    }
}

impl Rule {
    /// Builds a rule from the reference pattern source and a reference
    /// replacement template.
    ///
    /// This is the faithful entry point: `source` is what
    /// `RegExp.prototype.source` returns and `ignore_case` is the `/i` flag, so
    /// a rule ported from the reference keeps the reference's matching semantics —
    /// including `.` excluding all four line terminators and `/i` refusing to
    /// fold `ſ` into `s`. See [`crate::pattern`] for the details and the supported
    /// subset.
    ///
    /// The replacement uses the reference's `$` forms (`$1`, `$&`, `` $` ``, `$'`,
    /// `$$`), which are *not* the `regex` crate's: the reference reads `"$1s"` as
    /// group 1 followed by the letter `s`, whereas `Captures::expand` reads it
    /// as a group *named* `1s` and substitutes nothing.
    ///
    /// # Errors
    ///
    /// Returns [`PatternError`] if the pattern uses an unsupported construct or
    /// does not compile.
    pub fn from_pattern(
        source: &str,
        ignore_case: bool,
        replacement: impl Into<String>,
    ) -> Result<Self, PatternError> {
        let replacement = replacement.into();
        Ok(Self {
            kind: Kind::Pattern {
                re: pattern::compile(source, ignore_case)?,
                needs_captures: template_reads_groups(&replacement),
                replacement: Replacement::Template(Cow::Owned(replacement)),
            },
        })
    }

    /// Builds a rule from an already-compiled Rust regex.
    ///
    /// The escape hatch for callers who want the `regex` crate's own semantics
    /// (Unicode case folding, `.` excluding only `\n`) rather tha reference's.
    /// Prefer [`Rule::from_pattern`] when porting a rule from the reference.
    ///
    /// The replacement is still a reference template, because that is what the
    /// rest of the engine speaks.
    pub fn new(pattern: Regex, replacement: impl Into<String>) -> Self {
        let replacement = replacement.into();
        Self {
            kind: Kind::Pattern {
                re: pattern,
                needs_captures: template_reads_groups(&replacement),
                replacement: Replacement::Template(Cow::Owned(replacement)),
            },
        }
    }

    /// Compiles one entry of a built-in table.
    fn from_raw(raw: &'static RawRule) -> Self {
        if raw.source == MAN_PLURAL_SOURCE {
            return Self {
                kind: Kind::ManPlural,
            };
        }
        let re = pattern::compile(raw.source, true)
            .unwrap_or_else(|e| panic!("built-in rule /{}/i failed to translate: {e}", raw.source));
        let (replacement, needs_captures) = match raw.replacement {
            RawReplacement::Template(t) => (
                Replacement::Template(Cow::Borrowed(t)),
                template_reads_groups(t),
            ),
            RawReplacement::KeepIfListed { exceptions, suffix } => {
                // The Japanese function replacements all read group 1.
                (Replacement::KeepIfListed { exceptions, suffix }, true)
            }
        };
        Self {
            kind: Kind::Pattern {
                re,
                replacement,
                needs_captures,
            },
        }
    }

    /// Applies the rule, returning `Some` if the pattern matched.
    ///
    /// A match that rewrites to the empty string returns `Some("")`, not
    /// `None`: "matched" and "produced something usable" are different
    /// questions, and the engine's `||` chain distinguishes them. See
    /// [`crate::SingularPluralInflector`].
    #[must_use]
    pub fn apply(&self, token: &str) -> Option<String> {
        match &self.kind {
            Kind::ManPlural => apply_man_plural(token),
            Kind::Pattern {
                re,
                replacement,
                needs_captures: false,
            } => {
                // No group references: `find` reports the span without
                // allocating capture slots.
                let whole = re.find(token)?;
                let mut out = String::with_capacity(token.len() + 4);
                out.push_str(&token[..whole.start()]);
                match replacement {
                    Replacement::Template(t) => {
                        replace::expand(t, re, &MatchRef::Whole(whole), token, &mut out);
                    }
                    Replacement::KeepIfListed { exceptions, suffix } => {
                        push_keep_if_listed(&MatchRef::Whole(whole), exceptions, suffix, &mut out);
                    }
                }
                out.push_str(&token[whole.end()..]);
                Some(out)
            }
            Kind::Pattern {
                re,
                replacement,
                needs_captures: true,
            } => {
                // Test first: a rule that does not fire — eleven of twelve, for
                // a typical word — must not pay for capture slots.
                if !re.is_match(token) {
                    return None;
                }
                let caps = re.captures(token)?;
                let matched = MatchRef::Captured(&caps);
                let (start, end) = (matched.start(), matched.end());
                let mut out = String::with_capacity(token.len() + 4);
                out.push_str(&token[..start]);
                match replacement {
                    Replacement::Template(t) => {
                        replace::expand(t, re, &matched, token, &mut out);
                    }
                    Replacement::KeepIfListed { exceptions, suffix } => {
                        push_keep_if_listed(&matched, exceptions, suffix, &mut out);
                    }
                }
                out.push_str(&token[end..]);
                Some(out)
            }
        }
    }
}

/// The Japanese function replacements, which all share one shape:
///
/// ```text
/// function (a, mask) {
///   if (['い', 'おい', …].indexOf(mask) >= 0) { return mask + 'たち' }
///   return mask
/// }
/// ```
///
/// The pattern already required the suffix, so re-appending it leaves the word
/// unchanged — the list is a set of words that only *look* like plurals
/// (`かたち` "shape", `配達` "delivery"). The scan is linear, exactly like
/// `Array.indexOf`, over lists of at most 27 short strings; the `-達` list even
/// contains `伊` twice, which is harmless and kept verbatim.
fn push_keep_if_listed(
    matched: &MatchRef<'_, '_>,
    exceptions: &'static [&'static str],
    suffix: &'static str,
    out: &mut String,
) {
    let mask = matched.group(1).unwrap_or("");
    out.push_str(mask);
    if exceptions.contains(&mask) {
        out.push_str(suffix);
    }
}

/// The hand-written equivalent of `/^(?!talis|.*hu)(.*)man$/i → '$1men'`.
///
/// Decomposed from the reference, term by term:
///
/// * `(.*)man$` — `.` matches no line terminator and `$` is not multiline, so
///   the whole token must be one line ending in `man`. `.*` is greedy and `$`
///   anchors, so the *last* `man` is the one consumed: `manman` → `manmen`.
/// * `(?!talis…)` — the token must not start with `talis`.
/// * `(?!…|.*hu)` — `hu` must not occur anywhere `.*` can reach. Because a
///   token containing a line terminator already fails the main pattern, that
///   reduces to "`hu` occurs nowhere in the token".
/// * `/i` here is pure ASCII, and the reference's legacy canonicalisation folds
///   only ASCII letters into ASCII, so byte-wise ASCII comparison is exact.
///
/// Recorded behaviour this reproduces: `man`→`men`, `woman`→`women`,
/// `workman`→`workmen`, `human`→`humans` (rule declines, `(.*)`→`$1s` takes
/// over), `prehuman`→`prehumans`, `talisman`→`talismans`,
/// `xtalisman`→`xtalismen`, `shuman`→`shumans`, `huxman`→`huxmans`,
/// `talismanman`→`talismanmans`, `a\nhuman`→`as\nhuman`.
fn apply_man_plural(token: &str) -> Option<String> {
    if has_line_terminator(token) {
        return None;
    }
    let bytes = token.as_bytes();
    let stem_len = bytes.len().checked_sub(3)?;
    if !bytes[stem_len..].eq_ignore_ascii_case(b"man") {
        return None;
    }
    if bytes.len() >= 5 && bytes[..5].eq_ignore_ascii_case(b"talis") {
        return None;
    }
    if contains_hu(bytes) {
        return None;
    }
    // The tail is three ASCII bytes, so `stem_len` is a character boundary.
    let mut out = String::with_capacity(token.len() + 1);
    out.push_str(&token[..stem_len]);
    out.push_str("men");
    Some(out)
}

/// Whether the token contains any of the four characters the reference's `.`
/// refuses to match.
fn has_line_terminator(s: &str) -> bool {
    if s.is_ascii() {
        s.as_bytes().iter().any(|&b| b == b'\n' || b == b'\r')
    } else {
        s.chars()
            .any(|c| matches!(c, '\n' | '\r' | '\u{2028}' | '\u{2029}'))
    }
}

/// ASCII-case-insensitive substring test for `hu`.
///
/// `b | 0x20` is injective over each `{uppercase, lowercase}` pair and never
/// produces an ASCII letter from a UTF-8 continuation byte, so scanning raw
/// bytes cannot report a false hit inside a multi-byte character.
fn contains_hu(bytes: &[u8]) -> bool {
    bytes
        .windows(2)
        .any(|w| w[0] | 0x20 == b'h' && w[1] | 0x20 == b'u')
}

// ---------------------------------------------------------------------------
// Form sets
// ---------------------------------------------------------------------------

/// One direction's rules: `FormSet` in the reference.
pub(crate) struct FormSet {
    /// `regularForms`, in source order.
    pub(crate) regular: Vec<Rule>,
    /// `irregularForms`, sorted by key for binary search.
    irregular: &'static [(&'static str, &'static str)],
}

impl FormSet {
    fn build(raw: &'static [RawRule], irregular: &'static [(&'static str, &'static str)]) -> Self {
        Self {
            regular: raw.iter().map(Rule::from_raw).collect(),
            irregular,
        }
    }

    /// `formSet.irregularForms[token]`, where `token` is already lowercased.
    ///
    /// The reference stores these in an `Object.create(null)` map, so
    /// `'constructor'` and `'__proto__'` are ordinary absent keys rather than
    /// inherited functions — a `HashMap`-shaped lookup has the same property
    /// for free, and this binary search over a static table does too.
    pub(crate) fn irregular(&self, lower: &str) -> Option<&'static str> {
        self.irregular
            .binary_search_by_key(&lower, |(k, _)| k)
            .ok()
            .map(|i| self.irregular[i].1)
    }
}

/// Both directions of one inflector's built-in rules.
///
/// Built once per process behind a `LazyLock` and shared by every instance:
/// The reference rebuilds all of this in every constructor, but the tables are
/// immutable, and instance-local additions live separately in
/// [`crate::Core`](crate::inflector::Core).
pub(crate) struct Forms {
    pub(crate) plural: FormSet,
    pub(crate) singular: FormSet,
}

impl Forms {
    pub(crate) fn build(
        plural_regular: &'static [RawRule],
        plural_irregular: &'static [(&'static str, &'static str)],
        singular_regular: &'static [RawRule],
        singular_irregular: &'static [(&'static str, &'static str)],
    ) -> Self {
        Self {
            plural: FormSet::build(plural_regular, plural_irregular),
            singular: FormSet::build(singular_regular, singular_irregular),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn man_rule_fires_only_when_the_lookahead_allows_it() {
        let fires = |t: &str| apply_man_plural(t);
        assert_eq!(fires("man").as_deref(), Some("men"));
        assert_eq!(fires("woman").as_deref(), Some("women"));
        assert_eq!(fires("workman").as_deref(), Some("workmen"));
        assert_eq!(fires("MAN").as_deref(), Some("men"));
        assert_eq!(fires("Man").as_deref(), Some("men"));
        // Greedy `.*` consumes up to the LAST "man".
        assert_eq!(fires("manman").as_deref(), Some("manmen"));
        assert_eq!(fires("xtalisman").as_deref(), Some("xtalismen"));

        // Declined by the lookahead.
        assert_eq!(fires("human"), None);
        assert_eq!(fires("HUMAN"), None);
        assert_eq!(fires("prehuman"), None);
        assert_eq!(fires("shuman"), None);
        assert_eq!(fires("huxman"), None);
        assert_eq!(fires("talisman"), None);
        assert_eq!(fires("TALISMAN"), None);
        assert_eq!(fires("talismanman"), None);
        // Declined by the main pattern.
        assert_eq!(fires("ma"), None);
        assert_eq!(fires(""), None);
        assert_eq!(fires("a\nman"), None);
        assert_eq!(fires("a\rman"), None);
        assert_eq!(fires("a\u{2028}man"), None);
    }

    #[test]
    fn man_rule_handles_multibyte_stems() {
        assert_eq!(apply_man_plural("日本man").as_deref(), Some("日本men"));
        assert_eq!(apply_man_plural("👍man").as_deref(), Some("👍men"));
        assert_eq!(apply_man_plural("mañ"), None);
    }

    #[test]
    fn contains_hu_is_ascii_only_and_boundary_safe() {
        assert!(contains_hu(b"hu"));
        assert!(contains_hu(b"xHUx"));
        assert!(contains_hu(b"aHu"));
        assert!(!contains_hu(b"uh"));
        assert!(!contains_hu(b"h"));
        // No false hit from continuation bytes.
        assert!(!contains_hu("日本語".as_bytes()));
    }

    #[test]
    fn a_matching_rule_that_rewrites_to_nothing_still_reports_a_match() {
        let rule = Rule::from_pattern("^cat$", true, "").unwrap();
        assert_eq!(rule.apply("cat").as_deref(), Some(""));
        assert_eq!(rule.apply("dog"), None);
    }

    #[test]
    fn only_the_first_match_is_replaced() {
        // Non-global patterns rewrite once; `replace_all` would give "XX".
        let rule = Rule::from_pattern("a", true, "X").unwrap();
        assert_eq!(rule.apply("aa").as_deref(), Some("Xa"));
    }

    #[test]
    fn a_rust_regex_can_be_supplied_directly() {
        // The escape hatch keeps the regex crate's own semantics, which is the
        // documented difference from `from_pattern`: `(?i)s` folds U+017F here.
        let rule = Rule::new(Regex::new("(?i)s$").unwrap(), "$&es");
        assert_eq!(rule.apply("bus").as_deref(), Some("buses"));
        assert_eq!(rule.apply("bu\u{17f}").as_deref(), Some("bu\u{17f}es"));
        assert_eq!(
            Rule::from_pattern("s$", true, "$&es")
                .unwrap()
                .apply("bu\u{17f}"),
            None,
            "the reference entry point must not fold U+017F into `s`"
        );
    }

    #[test]
    fn unsupported_reference_constructs_surface_as_errors() {
        let err = Rule::from_pattern("(?=a)b", true, "x").unwrap_err();
        assert!(err.message().contains("lookahead"), "{err}");
    }
}
