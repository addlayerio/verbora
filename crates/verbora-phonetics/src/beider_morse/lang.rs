//! Language auto-detection: guesses which of a [`NameType`](crate::NameType)'s
//! languages a name is plausibly spelled under, purely from the spelling
//! itself — the `*_lang.txt` heuristic layer every reference implementation
//! runs before [`BeiderMorse::encode`](super::BeiderMorse::encode) decides
//! which rule file to load and which languages a candidate phoneme starts
//! out valid under. Not run at all by
//! [`BeiderMorse::encode_language`](super::BeiderMorse::encode_language),
//! which trusts the caller's own explicit language choice instead.

use regex::Regex;

use super::languages::LanguageSet;

struct LangRule {
    pattern: Regex,
    languages: LanguageSet,
    accept_on_match: bool,
}

pub(super) struct LangGuesser {
    all_languages: LanguageSet,
    rules: Vec<LangRule>,
}

impl LangGuesser {
    /// Guesses the language(s) `word` is plausibly spelled under, narrowing
    /// from "every language this name type has" one rule at a time, in file
    /// order: an "accept" rule intersects the running guess down to just its
    /// listed languages when its pattern matches; a "reject" rule removes
    /// its listed languages from the running guess instead. A guess that
    /// narrows all the way to nothing falls back to the full set — an empty
    /// result would mean "no candidate spelling is plausible under any
    /// language," which is never the intent of this heuristic.
    pub(super) fn guess(&self, word: &str) -> LanguageSet {
        let lower = word.to_lowercase();
        let mut langs = self.all_languages;
        for rule in &self.rules {
            if rule.pattern.is_match(&lower) {
                langs = if rule.accept_on_match {
                    langs.intersect(rule.languages)
                } else {
                    langs.difference(rule.languages)
                };
            }
        }
        if langs.is_empty() {
            self.all_languages
        } else {
            langs
        }
    }
}

/// Parses one already comment-stripped, non-blank `*_lang.txt` line:
/// `pattern langs true|false`, e.g. `rz$ polish+german true`. Several real
/// corpus lines carry a trailing `// ...` comment after the three fields —
/// handled for free by only ever reading the first three whitespace-
/// separated tokens and ignoring the rest.
fn parse_lang_rule(line: &str, resolve: &impl Fn(&str) -> Option<LanguageSet>) -> Option<LangRule> {
    let mut tokens = line.split_whitespace();
    let pattern = tokens.next()?;
    let langs = tokens.next()?;
    let accept_on_match = match tokens.next()? {
        "true" => true,
        "false" => false,
        _ => return None,
    };
    let mut languages = LanguageSet::EMPTY;
    for name in langs.split('+') {
        languages = languages.union(resolve(name)?);
    }
    Some(LangRule {
        pattern: Regex::new(pattern).ok()?,
        languages,
        accept_on_match,
    })
}

/// Compiles one `{prefix}_lang.txt` file's text into a [`LangGuesser`].
/// `resolve` maps a language name to its [`LanguageSet`] singleton — the
/// same lookup [`super::NameTypeData::resolve_language_name`] provides, so
/// unknown language names here (there should be none in the real corpus)
/// are treated as a malformed line and simply dropped rather than panicking:
/// this file is Verbora's own embedded data, not untrusted input, but a
/// silently-skipped stray rule is a far cheaper failure mode than a panic on
/// startup.
pub(super) fn compile(
    text: &'static str,
    all_languages: LanguageSet,
    resolve: impl Fn(&str) -> Option<LanguageSet>,
) -> LangGuesser {
    let mut rules = Vec::new();
    for line in super::meaningful_lines(text) {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with("//") {
            continue;
        }
        if let Some(rule) = parse_lang_rule(trimmed, &resolve) {
            rules.push(rule);
        }
    }
    LangGuesser {
        all_languages,
        rules,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::beider_morse::languages::Language;

    /// A tiny synthetic two-language universe (`"a"` = index 0, `"b"` =
    /// index 1) so these tests exercise [`LangGuesser`]'s own narrowing
    /// logic directly, independent of the real embedded `*_lang.txt`
    /// corpus (which is only ever exercised indirectly, through `mod.rs`'s
    /// end-to-end tests).
    fn resolve_ab(name: &str) -> Option<LanguageSet> {
        match name {
            "a" => Some(LanguageSet::single(Language(0))),
            "b" => Some(LanguageSet::single(Language(1))),
            _ => None,
        }
    }

    fn guesser(text: &'static str) -> LangGuesser {
        compile(text, LanguageSet::all(2), resolve_ab)
    }

    #[test]
    fn no_matching_rule_returns_the_full_set() {
        let g = guesser("xyz a true\n");
        assert_eq!(
            g.guess("word with none of those letters"),
            LanguageSet::all(2)
        );
    }

    #[test]
    fn accept_rule_narrows_to_its_listed_languages() {
        let g = guesser("z a true\n");
        assert_eq!(g.guess("zebra"), LanguageSet::single(Language(0)));
    }

    #[test]
    fn reject_rule_removes_its_listed_languages() {
        let g = guesser("z a false\n");
        // Starts from the full set, "z" rejects "a", leaving only "b".
        assert_eq!(g.guess("zebra"), LanguageSet::single(Language(1)));
    }

    #[test]
    fn rules_apply_in_file_order_not_first_match_only() {
        // Unlike the rule-application engine's "first match wins," the
        // language guesser runs *every* matching rule in file order,
        // narrowing further each time.
        let g = guesser("z a+b true\nq b true\n");
        assert_eq!(g.guess("zq"), LanguageSet::single(Language(1)));
    }

    #[test]
    fn narrowing_to_empty_falls_back_to_the_full_set() {
        // "z" accepts only "a", then "q" accepts only "b" -- the running
        // guess narrows to nothing, which falls back to the full set
        // rather than reporting "no language is plausible."
        let g = guesser("z a true\nq b true\n");
        assert_eq!(g.guess("zq"), LanguageSet::all(2));
    }

    #[test]
    fn trailing_comment_and_blank_lines_are_tolerated() {
        let g = guesser("\n// a comment\nz a true // trailing note\n");
        assert_eq!(g.guess("zebra"), LanguageSet::single(Language(0)));
    }
}
