//! Reading a [`Rule`](crate::Rule) back from its rule string. The grammar is
//! documented on [`Rule`](crate::Rule), since this module is private.

use std::fmt;

use crate::condition::Condition;
use crate::rule::{Rule, TagPattern};
use crate::tag::{LiteralError, Tag, Word};

/// Why a rule string did not parse.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum RuleParseError {
    /// Fewer than the three mandatory fields — old tag, new tag, condition name.
    TooFewFields {
        /// How many whitespace-separated fields the input had.
        found: usize,
    },
    /// The condition name is not one this crate defines.
    ///
    /// See [`Condition`] for the full table of names and aliases.
    UnknownCondition {
        /// The name as written.
        name: String,
    },
    /// The condition name is known but was given the wrong number of arguments.
    WrongArgumentCount {
        /// The canonical condition name.
        name: &'static str,
        /// How many arguments the condition takes.
        expected: usize,
        /// How many the input supplied.
        found: usize,
    },
    /// A field violated the [`Tag`]/[`Word`] literal contract.
    ///
    /// Only reachable for the empty field, since whitespace is what separates
    /// fields in the first place — but it is reported rather than assumed away.
    InvalidLiteral {
        /// Zero-based index of the offending field.
        field: usize,
        /// What was wrong with it.
        cause: LiteralError,
    },
    /// A `YES`/`NO` argument was spelled some other way.
    InvalidBoolean {
        /// Zero-based index of the offending field.
        field: usize,
        /// The argument as written.
        found: String,
    },
}

impl fmt::Display for RuleParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooFewFields { found } => write!(
                f,
                "a rule needs at least 3 fields (old tag, new tag, condition), found {found}"
            ),
            Self::UnknownCondition { name } => write!(f, "unknown condition {name:?}"),
            Self::WrongArgumentCount {
                name,
                expected,
                found,
            } => write!(
                f,
                "condition {name} takes {expected} argument(s), found {found}"
            ),
            Self::InvalidLiteral { field, cause } => {
                write!(f, "field {field}: {cause}")
            }
            Self::InvalidBoolean { field, found } => {
                write!(f, "field {field}: expected YES or NO, found {found:?}")
            }
        }
    }
}

impl std::error::Error for RuleParseError {}

impl From<(usize, LiteralError)> for RuleParseError {
    fn from((field, cause): (usize, LiteralError)) -> Self {
        Self::InvalidLiteral { field, cause }
    }
}

/// Parses one rule string.
///
/// # Errors
///
/// See [`RuleParseError`].
pub(crate) fn rule(input: &str) -> Result<Rule, RuleParseError> {
    let fields: Vec<&str> = input.split_whitespace().collect();
    if fields.len() < 3 {
        return Err(RuleParseError::TooFewFields {
            found: fields.len(),
        });
    }
    let from = if fields[0] == "*" {
        TagPattern::Any
    } else {
        TagPattern::Is(tag_field(fields[0], 0)?)
    };
    let to = tag_field(fields[1], 1)?;
    let condition = condition(fields[2], &fields[3..])?;
    Ok(Rule {
        from,
        to,
        condition,
    })
}

fn tag_field(s: &str, field: usize) -> Result<Tag, RuleParseError> {
    Tag::new(s.to_owned()).map_err(|cause| RuleParseError::InvalidLiteral { field, cause })
}

fn word_field(s: &str, field: usize) -> Result<Word, RuleParseError> {
    Word::new(s.to_owned()).map_err(|cause| RuleParseError::InvalidLiteral { field, cause })
}

fn bool_field(s: &str, field: usize) -> Result<bool, RuleParseError> {
    match s {
        "YES" => Ok(true),
        "NO" => Ok(false),
        _ => Err(RuleParseError::InvalidBoolean {
            field,
            found: s.to_owned(),
        }),
    }
}

/// Resolves a condition name to its canonical spelling and argument count.
///
/// `None` for a name this crate does not define.
fn lookup(name: &str) -> Option<(&'static str, usize)> {
    let canonical = match name {
        "PREV-TAG" | "PREVTAG" => "PREV-TAG",
        "NEXT-TAG" | "NEXTTAG" | "NEXT-WORD-IS-TAG" => "NEXT-TAG",
        "PREV-2-TAG" | "PREV2TAG" => "PREV-2-TAG",
        "NEXT-2-TAG" | "NEXT2TAG" => "NEXT-2-TAG",
        "PREV-1-OR-2-TAG" | "PREV1OR2TAG" => "PREV-1-OR-2-TAG",
        "NEXT-1-OR-2-TAG" | "NEXT1OR2TAG" => "NEXT-1-OR-2-TAG",
        "PREV-1-OR-2-OR-3-TAG" | "PREV1OR2OR3TAG" => "PREV-1-OR-2-OR-3-TAG",
        "NEXT-1-OR-2-OR-3-TAG" | "NEXT1OR2OR3TAG" => "NEXT-1-OR-2-OR-3-TAG",
        "SURROUND-TAG" | "SURROUNDTAG" => "SURROUND-TAG",
        "PREV-TAG-BIGRAM" | "PREVBIGRAM" => "PREV-TAG-BIGRAM",
        "NEXT-TAG-BIGRAM" | "NEXTBIGRAM" => "NEXT-TAG-BIGRAM",
        "CURRENT-WORD-IS" | "CURWD" => "CURRENT-WORD-IS",
        "PREV-WORD-IS" | "PREVWD" => "PREV-WORD-IS",
        "NEXT-WORD-IS" | "NEXTWD" => "NEXT-WORD-IS",
        "PREV-1-OR-2-WORD-IS" | "PREV1OR2WD" => "PREV-1-OR-2-WORD-IS",
        "NEXT-1-OR-2-WORD-IS" | "NEXT1OR2WD" => "NEXT-1-OR-2-WORD-IS",
        "LEFT-WORD-BIGRAM" | "LBIGRAM" => "LEFT-WORD-BIGRAM",
        "RIGHT-WORD-BIGRAM" | "RBIGRAM" => "RIGHT-WORD-BIGRAM",
        "WORD-AND-PREV-TAG" | "WDPREVTAG" => "WORD-AND-PREV-TAG",
        "WORD-AND-NEXT-TAG" | "WDNEXTTAG" => "WORD-AND-NEXT-TAG",
        "WORD-AND-2-TAG-BEFORE" | "WDAND2TAGBFR" => "WORD-AND-2-TAG-BEFORE",
        "WORD-AND-2-TAG-AFTER" | "WDAND2TAGAFT" => "WORD-AND-2-TAG-AFTER",
        "WORD-AND-2-WORD-AFTER" | "WDAND2AFT" => "WORD-AND-2-WORD-AFTER",
        "CURRENT-WORD-IS-CAP" => "CURRENT-WORD-IS-CAP",
        "NEXT-WORD-IS-CAP" => "NEXT-WORD-IS-CAP",
        "PREV-WORD-IS-CAP" => "PREV-WORD-IS-CAP",
        "CURRENT-WORD-IS-NUMBER" => "CURRENT-WORD-IS-NUMBER",
        "CURRENT-WORD-IS-URL" => "CURRENT-WORD-IS-URL",
        "CURRENT-WORD-ENDS-WITH" => "CURRENT-WORD-ENDS-WITH",
        _ => return None,
    };
    let arity = match canonical {
        "SURROUND-TAG"
        | "PREV-TAG-BIGRAM"
        | "NEXT-TAG-BIGRAM"
        | "LEFT-WORD-BIGRAM"
        | "RIGHT-WORD-BIGRAM"
        | "WORD-AND-PREV-TAG"
        | "WORD-AND-NEXT-TAG"
        | "WORD-AND-2-TAG-BEFORE"
        | "WORD-AND-2-TAG-AFTER"
        | "WORD-AND-2-WORD-AFTER" => 2,
        _ => 1,
    };
    Some((canonical, arity))
}

/// Every condition name this crate accepts, canonical spellings and aliases,
/// for the enumeration tests.
#[cfg(test)]
pub(crate) const ACCEPTED_NAMES: &[&str] = &[
    "PREV-TAG",
    "PREVTAG",
    "NEXT-TAG",
    "NEXTTAG",
    "NEXT-WORD-IS-TAG",
    "PREV-2-TAG",
    "PREV2TAG",
    "NEXT-2-TAG",
    "NEXT2TAG",
    "PREV-1-OR-2-TAG",
    "PREV1OR2TAG",
    "NEXT-1-OR-2-TAG",
    "NEXT1OR2TAG",
    "PREV-1-OR-2-OR-3-TAG",
    "PREV1OR2OR3TAG",
    "NEXT-1-OR-2-OR-3-TAG",
    "NEXT1OR2OR3TAG",
    "SURROUND-TAG",
    "SURROUNDTAG",
    "PREV-TAG-BIGRAM",
    "PREVBIGRAM",
    "NEXT-TAG-BIGRAM",
    "NEXTBIGRAM",
    "CURRENT-WORD-IS",
    "CURWD",
    "PREV-WORD-IS",
    "PREVWD",
    "NEXT-WORD-IS",
    "NEXTWD",
    "PREV-1-OR-2-WORD-IS",
    "PREV1OR2WD",
    "NEXT-1-OR-2-WORD-IS",
    "NEXT1OR2WD",
    "LEFT-WORD-BIGRAM",
    "LBIGRAM",
    "RIGHT-WORD-BIGRAM",
    "RBIGRAM",
    "WORD-AND-PREV-TAG",
    "WDPREVTAG",
    "WORD-AND-NEXT-TAG",
    "WDNEXTTAG",
    "WORD-AND-2-TAG-BEFORE",
    "WDAND2TAGBFR",
    "WORD-AND-2-TAG-AFTER",
    "WDAND2TAGAFT",
    "WORD-AND-2-WORD-AFTER",
    "WDAND2AFT",
    "CURRENT-WORD-IS-CAP",
    "NEXT-WORD-IS-CAP",
    "PREV-WORD-IS-CAP",
    "CURRENT-WORD-IS-NUMBER",
    "CURRENT-WORD-IS-URL",
    "CURRENT-WORD-ENDS-WITH",
];

fn condition(name: &str, args: &[&str]) -> Result<Condition, RuleParseError> {
    let Some((canonical, arity)) = lookup(name) else {
        return Err(RuleParseError::UnknownCondition {
            name: name.to_owned(),
        });
    };
    if args.len() != arity {
        return Err(RuleParseError::WrongArgumentCount {
            name: canonical,
            expected: arity,
            found: args.len(),
        });
    }
    // Argument fields start at index 3 of the rule string.
    let a = |k: usize| args[k];
    let f = |k: usize| k + 3;
    let t = |k: usize| tag_field(a(k), f(k));
    let w = |k: usize| word_field(a(k), f(k));
    let b = |k: usize| bool_field(a(k), f(k));

    Ok(match canonical {
        "PREV-TAG" => Condition::PrevTag(t(0)?),
        "NEXT-TAG" => Condition::NextTag(t(0)?),
        "PREV-2-TAG" => Condition::PrevTag2(t(0)?),
        "NEXT-2-TAG" => Condition::NextTag2(t(0)?),
        "PREV-1-OR-2-TAG" => Condition::PrevTagWithin2(t(0)?),
        "NEXT-1-OR-2-TAG" => Condition::NextTagWithin2(t(0)?),
        "PREV-1-OR-2-OR-3-TAG" => Condition::PrevTagWithin3(t(0)?),
        "NEXT-1-OR-2-OR-3-TAG" => Condition::NextTagWithin3(t(0)?),
        "SURROUND-TAG" => Condition::SurroundingTags {
            prev: t(0)?,
            next: t(1)?,
        },
        "PREV-TAG-BIGRAM" => Condition::PrevTagBigram {
            two_before: t(0)?,
            before: t(1)?,
        },
        "NEXT-TAG-BIGRAM" => Condition::NextTagBigram {
            after: t(0)?,
            two_after: t(1)?,
        },
        "CURRENT-WORD-IS" => Condition::CurrentWord(w(0)?),
        "PREV-WORD-IS" => Condition::PrevWord(w(0)?),
        "NEXT-WORD-IS" => Condition::NextWord(w(0)?),
        "PREV-1-OR-2-WORD-IS" => Condition::PrevWordWithin2(w(0)?),
        "NEXT-1-OR-2-WORD-IS" => Condition::NextWordWithin2(w(0)?),
        "LEFT-WORD-BIGRAM" => Condition::LeftWordBigram {
            before: w(0)?,
            current: w(1)?,
        },
        "RIGHT-WORD-BIGRAM" => Condition::RightWordBigram {
            current: w(0)?,
            after: w(1)?,
        },
        "WORD-AND-PREV-TAG" => Condition::CurrentWordAndPrevTag {
            prev_tag: t(0)?,
            word: w(1)?,
        },
        "WORD-AND-NEXT-TAG" => Condition::CurrentWordAndNextTag {
            word: w(0)?,
            next_tag: t(1)?,
        },
        "WORD-AND-2-TAG-BEFORE" => Condition::CurrentWordAndTag2Before {
            tag_two_before: t(0)?,
            word: w(1)?,
        },
        "WORD-AND-2-TAG-AFTER" => Condition::CurrentWordAndTag2After {
            word: w(0)?,
            tag_two_after: t(1)?,
        },
        "WORD-AND-2-WORD-AFTER" => Condition::CurrentWordAndWord2After {
            word: w(0)?,
            word_two_after: w(1)?,
        },
        "CURRENT-WORD-IS-CAP" => Condition::CurrentWordIsCapitalized(b(0)?),
        "NEXT-WORD-IS-CAP" => Condition::NextWordIsCapitalized(b(0)?),
        "PREV-WORD-IS-CAP" => Condition::PrevWordIsCapitalized(b(0)?),
        "CURRENT-WORD-IS-NUMBER" => Condition::CurrentWordIsNumeral(b(0)?),
        "CURRENT-WORD-IS-URL" => Condition::CurrentWordLooksLikeUrl(b(0)?),
        "CURRENT-WORD-ENDS-WITH" => Condition::CurrentWordEndsWith(w(0)?),
        other => unreachable!("lookup returned an unhandled canonical name {other:?}"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    #[test]
    fn fields_split_on_any_whitespace_run() {
        let want = Rule::from_str("NN VB PREV-TAG TO").unwrap();
        for source in [
            "NN VB PREV-TAG TO",
            "  NN VB PREV-TAG TO  ",
            "NN\tVB\tPREV-TAG\tTO",
            "NN   VB \n PREV-TAG\u{00a0}TO",
        ] {
            assert_eq!(Rule::from_str(source).unwrap(), want, "{source:?}");
        }
    }

    #[test]
    fn too_few_fields() {
        for (source, found) in [("", 0), ("NN", 1), ("NN VB", 2), ("   ", 0)] {
            assert_eq!(
                Rule::from_str(source),
                Err(RuleParseError::TooFewFields { found }),
                "{source:?}"
            );
        }
    }

    /// An unrecognised condition name is an error, not a rule that silently
    /// never fires.
    #[test]
    fn unknown_conditions_are_rejected() {
        for name in ["NOPE", "prev-tag", "toString", "__proto__", "DEFAULT"] {
            assert_eq!(
                Rule::from_str(&format!("A B {name} x")),
                Err(RuleParseError::UnknownCondition {
                    name: name.to_owned()
                }),
                "{name}"
            );
        }
    }

    #[test]
    fn argument_counts_are_enforced() {
        assert_eq!(
            Rule::from_str("A B PREV-TAG"),
            Err(RuleParseError::WrongArgumentCount {
                name: "PREV-TAG",
                expected: 1,
                found: 0
            })
        );
        assert_eq!(
            Rule::from_str("A B PREV-TAG X Y"),
            Err(RuleParseError::WrongArgumentCount {
                name: "PREV-TAG",
                expected: 1,
                found: 2
            })
        );
        assert_eq!(
            Rule::from_str("A B SURROUND-TAG X"),
            Err(RuleParseError::WrongArgumentCount {
                name: "SURROUND-TAG",
                expected: 2,
                found: 1
            })
        );
    }

    #[test]
    fn boolean_arguments_must_be_yes_or_no() {
        assert_eq!(
            Rule::from_str("A B CURRENT-WORD-IS-CAP maybe"),
            Err(RuleParseError::InvalidBoolean {
                field: 3,
                found: "maybe".to_owned()
            })
        );
        assert_eq!(
            Rule::from_str("A B CURRENT-WORD-IS-CAP yes"),
            Err(RuleParseError::InvalidBoolean {
                field: 3,
                found: "yes".to_owned()
            })
        );
        assert!(Rule::from_str("A B CURRENT-WORD-IS-CAP YES").is_ok());
        assert!(Rule::from_str("A B CURRENT-WORD-IS-CAP NO").is_ok());
    }

    /// Every accepted name resolves, and every alias resolves to the same
    /// condition its canonical spelling does. Enumerated over the whole table.
    #[test]
    fn every_accepted_name_parses_and_aliases_agree() {
        for name in ACCEPTED_NAMES {
            let (canonical, arity) = lookup(name).unwrap_or_else(|| panic!("{name} not found"));
            let args = match (canonical, arity) {
                (_, 1) if canonical.ends_with("-CAP") => "YES".to_owned(),
                ("CURRENT-WORD-IS-NUMBER" | "CURRENT-WORD-IS-URL", _) => "YES".to_owned(),
                (_, 1) => "X".to_owned(),
                (_, _) => "X Y".to_owned(),
            };
            let via_alias = Rule::from_str(&format!("A B {name} {args}")).unwrap();
            let via_canonical = Rule::from_str(&format!("A B {canonical} {args}")).unwrap();
            assert_eq!(via_alias, via_canonical, "alias {name}");
            assert_eq!(via_alias.condition.name(), canonical);
        }
    }

    /// Whichever name a rule was written with, writing it out and reading it
    /// back yields the same rule.
    #[test]
    fn canonical_output_reparses_to_the_same_rule() {
        for name in ACCEPTED_NAMES {
            let (canonical, arity) = lookup(name).expect("in table");
            let args = if canonical.ends_with("-CAP")
                || canonical == "CURRENT-WORD-IS-NUMBER"
                || canonical == "CURRENT-WORD-IS-URL"
            {
                "NO".to_owned()
            } else if arity == 1 {
                "X".to_owned()
            } else {
                "X Y".to_owned()
            };
            let rule = Rule::from_str(&format!("* B {name} {args}")).unwrap();
            assert_eq!(rule.to_string().parse::<Rule>().unwrap(), rule, "{name}");
        }
    }
}
