//! The sentence classification produced by [`SentenceAnalyzer::type_of`].
//!
//! [`SentenceAnalyzer::type_of`]: crate::SentenceAnalyzer::type_of

use std::fmt;
use std::str::FromStr;

/// How a sentence is classified.
///
/// Ports the reference `SenType.ts`, which is a TypeScript
/// `const enum` — type-only, fully inlined at compile time, with **no runtime
/// object emitted**. The reference writes the bare string literals below into
/// `analyzer.senType`, so those strings, not the member names, are the
/// observable contract; [`SenType::as_str`] and the [`Display`] impl produce
/// them.
///
/// [`Display`]: std::fmt::Display
///
/// ```
/// use verbora_analyzers::SenType;
///
/// assert_eq!(SenType::Interrogative.as_str(), "INTERROGATIVE");
/// assert_eq!("COMMAND".parse(), Ok(SenType::Command));
/// ```
///
/// # There is a sixth state, and it is not in this enum
///
/// `analyzer.senType` starts as `null` and the `switch` in `type()` has no
/// default arm, so a sentence can be analysed and still have no type. That is
/// why the analyzer's field and return values are `Option<SenType>` rather than
/// `SenType`; see [`SentenceAnalyzer::type_of`] for the exact input that
/// reaches it.
///
/// [`SentenceAnalyzer::type_of`]: crate::SentenceAnalyzer::type_of
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum SenType {
    /// `'UNKNOWN'` — no terminal punctuation and nothing else matched.
    Unknown,
    /// `'COMMAND'` — an imperative, detected by the implicit-'You' subject.
    Command,
    /// `'INTERROGATIVE'` — a question.
    Interrogative,
    /// `'EXCLAMATORY'` — ends in `!` and is not a command.
    Exclamatory,
    /// `'DECLARATIVE'` — ends in `.` and is not a command.
    Declarative,
}

impl SenType {
    /// Every variant, in the declaration order of the TypeScript `const enum`.
    pub const ALL: [Self; 5] = [
        Self::Unknown,
        Self::Command,
        Self::Interrogative,
        Self::Exclamatory,
        Self::Declarative,
    ];

    /// The exact string the reference stores in `analyzer.senType`.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Unknown => "UNKNOWN",
            Self::Command => "COMMAND",
            Self::Interrogative => "INTERROGATIVE",
            Self::Exclamatory => "EXCLAMATORY",
            Self::Declarative => "DECLARATIVE",
        }
    }
}

impl fmt::Display for SenType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// The error returned when a string is not one of the five [`SenType`] values.
///
/// Parsing is case-sensitive because the reference comparison is: the library
/// writes uppercase literals and callers compare them with `===`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseSenTypeError(
    /// The string that failed to parse.
    pub String,
);

impl fmt::Display for ParseSenTypeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "not a sentence type: {:?}", self.0)
    }
}

impl std::error::Error for ParseSenTypeError {}

impl FromStr for SenType {
    type Err = ParseSenTypeError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "UNKNOWN" => Ok(Self::Unknown),
            "COMMAND" => Ok(Self::Command),
            "INTERROGATIVE" => Ok(Self::Interrogative),
            "EXCLAMATORY" => Ok(Self::Exclamatory),
            "DECLARATIVE" => Ok(Self::Declarative),
            other => Err(ParseSenTypeError(other.to_owned())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strings_round_trip() {
        for t in SenType::ALL {
            assert_eq!(t.as_str().parse(), Ok(t));
            assert_eq!(t.to_string(), t.as_str());
        }
    }

    #[test]
    fn parsing_is_case_sensitive() {
        assert!("command".parse::<SenType>().is_err());
        assert!("".parse::<SenType>().is_err());
        assert!("COMMANDS".parse::<SenType>().is_err());
    }

    #[test]
    fn error_names_the_input() {
        let e = "nope".parse::<SenType>().unwrap_err();
        assert_eq!(e.to_string(), r#"not a sentence type: "nope""#);
    }
}
