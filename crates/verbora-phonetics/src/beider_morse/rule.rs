//! The Beider-Morse rule DSL: parsing `"pattern" "left" "right" "phonetic"`
//! quadruplets (plus `#include`, comments, and per-language files) into
//! [`Rule`]s, and the phonetic-output side (`"(a|b[lang]|c)"`) into
//! [`PhonemeExpr`].
//!
//! The grammar here is deliberately hand-written, not built on a parser
//! combinator crate — it is small and line-oriented (confirmed against the
//! real rule corpus in `data/beider-morse/`, and cross-checked against the
//! grammar `rphonetic`'s own `nom`-based parser implements), and this
//! project's own convention elsewhere (`verbora-tagger`'s lexicon format,
//! `verbora-phonetics`'s own Daitch-Mokotoff table) is a small hand-rolled
//! parser over `nom`/similar for exactly this class of format.

use std::borrow::Cow;

use regex::Regex;

use super::languages::LanguageSet;

/// One alternative in a rule's phonetic output: literal text, optionally
/// restricted to a single language (`"in[russian]"` — `Some(language
/// index)`) or valid under every language in scope (`"ina"` — `None`, the
/// DSL's own "untagged" meaning, resolved to [`LanguageSet::all`] by the
/// caller once the active [`NameType`](crate::NameType)'s language count is
/// known).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct PhonemeAlt<'a> {
    pub(super) text: Cow<'a, str>,
    /// `None` = untagged (every language). `Some(name)` = restricted to the
    /// single named language — resolved against a [`NameType`](crate::NameType)'s
    /// own language table by the loader, not here (this module has no
    /// notion of *which* name type it's parsing for).
    pub(super) language: Option<&'a str>,
}

/// A rule's phonetic-output field, parsed: one or more alternatives.
/// `"(a|b[lang]|c)"` and the un-parenthesized single-alternative shorthand
/// `"a"` (including the empty string, meaning "this pattern consumes input
/// and contributes nothing") both parse to this same shape.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct PhonemeExpr<'a>(pub(super) Vec<PhonemeAlt<'a>>);

/// Parses a phonetic-output field like `"(in[russian]|ina)"` or `""` or
/// `"mb"`.
pub(super) fn parse_phoneme_expr(field: &str) -> PhonemeExpr<'_> {
    let inner = field
        .strip_prefix('(')
        .and_then(|s| s.strip_suffix(')'))
        .unwrap_or(field);
    let alts = inner
        .split('|')
        .map(|alt| {
            if let Some(open) = alt.find('[')
                && alt.ends_with(']')
            {
                PhonemeAlt {
                    text: Cow::Borrowed(&alt[..open]),
                    language: Some(&alt[open + 1..alt.len() - 1]),
                }
            } else {
                PhonemeAlt {
                    text: Cow::Borrowed(alt),
                    language: None,
                }
            }
        })
        .collect();
    PhonemeExpr(alts)
}

/// One parsed line of rule data: `"pattern" "left_context" "right_context" "phonetic"`.
///
/// `left_context`/`right_context` are regexes matched against the text
/// immediately before/after `pattern`'s match position — `left_context` is
/// matched against the input up to (not including) the pattern, anchored at
/// its *end*; `right_context` is matched against the input starting right
/// after the pattern, anchored at its *start*. An empty context string means
/// "always matches" (compiled as `.*`/kept unanchored-empty, per
/// `rphonetic`'s own treatment — see `Rule::compile`'s doc comment).
#[derive(Debug)]
pub(super) struct RawRule<'a> {
    pub(super) pattern: &'a str,
    pub(super) left_context: &'a str,
    pub(super) right_context: &'a str,
    pub(super) phonetic: &'a str,
}

/// One physical line of a rule file, after comment/blank-line filtering.
pub(super) enum Line<'a> {
    Rule(RawRule<'a>),
    Include(&'a str),
    /// A single bare identifier line — used by `*_languages.txt` (one
    /// language name per line) and `*_lang.txt` (handled separately, see
    /// [`super::lang`]).
    Word(&'a str),
}

/// Parses one non-comment, non-blank line of a rule file.
///
/// Returns `None` for lines this parser does not need to understand for the
/// files it is asked to read (defensive — callers pass only file kinds they
/// expect one of the three [`Line`] shapes from).
pub(super) fn parse_line(line: &str) -> Option<Line<'_>> {
    let line = line.trim();
    if line.is_empty() || line.starts_with("//") {
        return None;
    }
    if let Some(name) = line.strip_prefix("#include ") {
        return Some(Line::Include(name.trim()));
    }
    if line.starts_with('"') {
        return parse_quadruplet(line).map(Line::Rule);
    }
    Some(Line::Word(line))
}

/// Parses `"a" "b" "c" "d"`, tolerating a trailing `// comment` and the
/// variable inter-field whitespace (spaces and/or tabs) the real corpus
/// uses. `\"` inside a field is the one escape the DSL defines (confirmed
/// against `rphonetic`'s own grammar and the real corpus, which uses it for
/// a literal double-quote pattern) — the returned field text still carries
/// the raw `\"`, unescaped; see [`unescape_quote`], applied once each field
/// reaches [`Rule::compile`] rather than here, since every field here is a
/// zero-copy borrow into the source line and unescaping would force an
/// allocation on every field regardless of whether it needs one.
fn parse_quadruplet(line: &str) -> Option<RawRule<'_>> {
    let mut rest = line;
    let mut fields = [""; 4];
    for field in &mut fields {
        rest = rest.strip_prefix('"')?;
        let end = find_unescaped_quote(rest)?;
        *field = &rest[..end];
        rest = rest[end + 1..].trim_start();
    }
    let [pattern, left_context, right_context, phonetic] = fields;
    Some(RawRule {
        pattern,
        left_context,
        right_context,
        phonetic,
    })
}

/// The index of the first `"` not immediately preceded by `\`.
fn find_unescaped_quote(s: &str) -> Option<usize> {
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'"' && (i == 0 || bytes[i - 1] != b'\\') {
            return Some(i);
        }
        i += 1;
    }
    None
}

/// Replaces `\"` with a literal `"` — the one escape [`parse_quadruplet`]'s
/// own field-boundary scan (`find_unescaped_quote`) recognizes but does not
/// itself apply. Borrows when there is nothing to unescape (every field
/// that isn't one of the real corpus's four `"\"" "" "" ""`-shaped lines),
/// so parsing a rule file that never uses the escape allocates nothing here.
fn unescape_quote(s: &str) -> Cow<'_, str> {
    if s.contains('\\') {
        Cow::Owned(s.replace("\\\"", "\""))
    } else {
        Cow::Borrowed(s)
    }
}

/// A compiled rule: a literal pattern plus compiled left/right context
/// regexes, ready to test against an input position.
pub(super) struct Rule {
    pub(super) pattern: String,
    left_context: Option<Regex>,
    right_context: Option<Regex>,
    pub(super) phonemes: Vec<(Cow<'static, str>, LanguageSet)>,
}

impl Rule {
    /// Compiles a [`RawRule`] against an already-resolved language table
    /// (untagged phoneme alternatives get `all_languages`; tagged ones get
    /// the single matching language's own [`LanguageSet`], or are silently
    /// dropped — no warning, debug or otherwise — if the tag names a
    /// language this name type's table does not have. A rule whose *every*
    /// alternative fails to resolve this way would collapse that whole
    /// in-progress candidate branch to nothing with no signal at all; the
    /// `corpus_language_tags_all_resolve` test in this module's parent
    /// (`mod.rs`) is the regression guard that the real embedded corpus
    /// never actually hits this.
    pub(super) fn compile(
        raw: &RawRule<'_>,
        all_languages: LanguageSet,
        resolve_language: impl Fn(&str) -> Option<LanguageSet>,
    ) -> Result<Self, regex::Error> {
        let left_context = unescape_quote(raw.left_context);
        let left_context = if left_context.is_empty() {
            None
        } else {
            Some(Regex::new(&format!("({left_context})$"))?)
        };
        let right_context = unescape_quote(raw.right_context);
        let right_context = if right_context.is_empty() {
            None
        } else {
            Some(Regex::new(&format!("^({right_context})"))?)
        };
        let phonetic = unescape_quote(raw.phonetic);
        let expr = parse_phoneme_expr(&phonetic);
        let phonemes = expr
            .0
            .into_iter()
            .filter_map(|alt| {
                let langs = match alt.language {
                    None => all_languages,
                    // A tag can list more than one language, `+`-joined
                    // (e.g. `gv[portuguese+spanish]`) -- same compound-tag
                    // shape `super::lang`'s guesser rules use. Union each
                    // name's resolved set rather than looking up the whole
                    // tag as one name, which would never match and silently
                    // drop the entire alternative.
                    Some(tag) => tag.split('+').try_fold(LanguageSet::EMPTY, |acc, name| {
                        resolve_language(name).map(|l| acc.union(l))
                    })?,
                };
                Some((Cow::Owned(alt.text.into_owned()), langs))
            })
            .collect();
        Ok(Self {
            pattern: unescape_quote(raw.pattern).into_owned(),
            left_context,
            right_context,
            phonemes,
        })
    }

    /// Whether this rule's pattern, left context and right context all
    /// match `input` at byte offset `at`.
    pub(super) fn matches(&self, input: &str, at: usize) -> bool {
        if !input[at..].starts_with(self.pattern.as_str()) {
            return false;
        }
        let matches_left = self
            .left_context
            .as_ref()
            .is_none_or(|re| re.is_match(&input[..at]));
        let matches_right = self
            .right_context
            .as_ref()
            .is_none_or(|re| re.is_match(&input[at + self.pattern.len()..]));
        matches_left && matches_right
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_simple_quadruplet() {
        let Some(Line::Rule(r)) = parse_line(r#""yna" "" "$" "(in[russian]|ina)""#) else {
            panic!("expected a rule line");
        };
        assert_eq!(r.pattern, "yna");
        assert_eq!(r.left_context, "");
        assert_eq!(r.right_context, "$");
        assert_eq!(r.phonetic, "(in[russian]|ina)");
    }

    #[test]
    fn compile_unescapes_a_literal_quote_pattern() {
        // The real corpus's own shape for this escape, verbatim (e.g.
        // `ash_rules_russian.txt`, `gen_rules_any.txt`): a rule matching a
        // literal `"` character and contributing nothing.
        let Some(Line::Rule(raw)) = parse_line(r#""\"" "" "" """#) else {
            panic!("expected a rule line");
        };
        assert_eq!(
            raw.pattern, "\\\"",
            "the raw field still carries the escape"
        );
        let rule = Rule::compile(&raw, LanguageSet::all(1), |_| None).unwrap();
        assert_eq!(
            rule.pattern, "\"",
            "compiled pattern is the unescaped literal quote"
        );
        assert!(rule.matches("a\"b", 1));
    }

    #[test]
    fn parses_quadruplet_with_trailing_comment() {
        let Some(Line::Rule(r)) = parse_line(r#""a" "" "" "b"   // a comment"#) else {
            panic!("expected a rule line");
        };
        assert_eq!(r.phonetic, "b");
    }

    #[test]
    fn parses_include() {
        let Some(Line::Include(name)) = parse_line("#include gen_approx_russian") else {
            panic!("expected an include line");
        };
        assert_eq!(name, "gen_approx_russian");
    }

    #[test]
    fn skips_comments_and_blank_lines() {
        assert!(parse_line("").is_none());
        assert!(parse_line("   ").is_none());
        assert!(parse_line("// a comment").is_none());
    }

    #[test]
    fn parses_bare_word_line() {
        let Some(Line::Word(w)) = parse_line("russian") else {
            panic!("expected a word line");
        };
        assert_eq!(w, "russian");
    }

    #[test]
    fn phoneme_expr_parses_untagged_alternatives() {
        let expr = parse_phoneme_expr("(lova|lof[russian]|lef[russian])");
        assert_eq!(expr.0.len(), 3);
        assert_eq!(expr.0[0].text, "lova");
        assert_eq!(expr.0[0].language, None);
        assert_eq!(expr.0[1].text, "lof");
        assert_eq!(expr.0[1].language, Some("russian"));
    }

    #[test]
    fn phoneme_expr_parses_single_value_without_parens() {
        let expr = parse_phoneme_expr("mb");
        assert_eq!(expr.0.len(), 1);
        assert_eq!(expr.0[0].text, "mb");
    }

    #[test]
    fn phoneme_expr_parses_empty_string() {
        let expr = parse_phoneme_expr("");
        assert_eq!(expr.0.len(), 1);
        assert_eq!(expr.0[0].text, "");
    }

    #[test]
    fn rule_matches_respects_left_and_right_context() {
        let raw = RawRule {
            pattern: "a",
            left_context: "^",
            right_context: "b",
            phonetic: "X",
        };
        let rule = Rule::compile(&raw, LanguageSet::all(1), |_| None).unwrap();
        assert!(rule.matches("ab", 0));
        assert!(!rule.matches("ba", 1)); // not at start
        assert!(!rule.matches("ac", 0)); // wrong right context
    }
}
