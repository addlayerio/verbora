use std::fmt;
use std::str::FromStr;

/// The four clause types of English, as a rule-based analyzer can distinguish
/// them.
///
/// The names and the division are the standard grammatical ones: declarative,
/// interrogative, imperative and exclamative (Quirk, Greenbaum, Leech &
/// Svartvik, *A Comprehensive Grammar of the English Language*, 1985, §11.1;
/// Huddleston & Pullum, *The Cambridge Grammar of the English Language*, 2002,
/// ch. 10).
///
/// # There is no fifth "unknown" variant
///
/// Absence of evidence is [`Option::None`], not a member of this enum:
/// [`SentenceAnalysis::sentence_type`] returns `Option<SentenceType>`. A
/// sentence with no terminal punctuation and no clause-level cue is genuinely
/// unclassified, and a `SentenceType::Unknown` sentinel would let that state
/// travel through code paths that expect a real type.
///
/// ```
/// use verbora_analyzers::{SentenceType, TaggedWord as W, analyze};
///
/// // A full stop makes this declarative.
/// let ended = [W::new("It", "PRP"), W::new("rained", "VBD"), W::new(".", ".")];
/// assert_eq!(analyze(&ended).sentence_type(), Some(SentenceType::Declarative));
///
/// // The same words without it carry no cue at all.
/// let bare = [W::new("It", "PRP"), W::new("rained", "VBD")];
/// assert_eq!(analyze(&bare).sentence_type(), None);
/// ```
///
/// [`SentenceAnalysis::sentence_type`]: crate::SentenceAnalysis::sentence_type
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum SentenceType {
    /// A statement — *The bear chased the squirrel.*
    Declarative,
    /// A question — *Who voted?*, *Should we go?*
    Interrogative,
    /// A command or request — *Vote for me.*
    ///
    /// English imperatives have no overt subject; the analyzer reports the
    /// understood one as [`ImpliedSubject::SecondPerson`].
    ///
    /// [`ImpliedSubject::SecondPerson`]: crate::ImpliedSubject::SecondPerson
    Imperative,
    /// An exclamation — *What a day!*
    Exclamative,
}

impl SentenceType {
    /// Every type, in declaration order.
    pub const ALL: [Self; 4] = [
        Self::Declarative,
        Self::Interrogative,
        Self::Imperative,
        Self::Exclamative,
    ];

    /// The canonical name, lowercase.
    ///
    /// This is the exact string [`FromStr`] accepts and [`fmt::Display`]
    /// writes, so `t.as_str().parse() == Ok(t)` for every variant.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Declarative => "declarative",
            Self::Interrogative => "interrogative",
            Self::Imperative => "imperative",
            Self::Exclamative => "exclamative",
        }
    }
}

impl fmt::Display for SentenceType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Returned by [`SentenceType`]'s [`FromStr`] when the input is not one of the
/// four canonical names.
///
/// ```
/// use verbora_analyzers::SentenceType;
///
/// let error = "Declarative".parse::<SentenceType>().unwrap_err();
/// assert_eq!(error.input(), "Declarative");
/// ```
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ParseSentenceTypeError {
    input: String,
}

impl ParseSentenceTypeError {
    /// The string that failed to parse, unchanged.
    #[must_use]
    pub fn input(&self) -> &str {
        &self.input
    }
}

impl fmt::Display for ParseSentenceTypeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "not a sentence type: {:?}", self.input)
    }
}

impl std::error::Error for ParseSentenceTypeError {}

/// Parses one of the four canonical names.
///
/// Matching is **exact**: the parser neither folds case nor trims whitespace,
/// because doing either would accept inputs that do not round-trip through
/// [`SentenceType::as_str`]. Callers that want a lenient parse should fold or
/// trim explicitly before calling.
impl FromStr for SentenceType {
    type Err = ParseSentenceTypeError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::ALL
            .into_iter()
            .find(|t| t.as_str() == s)
            .ok_or_else(|| ParseSentenceTypeError {
                input: s.to_owned(),
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Walks every variant: name, round trip, and `Display` agreement.
    #[test]
    fn every_variant_round_trips() {
        assert_eq!(SentenceType::ALL.len(), 4);
        for t in SentenceType::ALL {
            assert_eq!(t.as_str().parse(), Ok(t), "{t}");
            assert_eq!(t.to_string(), t.as_str(), "{t}");
        }
    }

    #[test]
    fn names_are_the_standard_grammatical_terms_and_are_distinct() {
        let names: Vec<&str> = SentenceType::ALL.iter().map(|t| t.as_str()).collect();
        assert_eq!(
            names,
            ["declarative", "interrogative", "imperative", "exclamative"]
        );
        let mut sorted = names.clone();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.len(), names.len());
    }

    #[test]
    fn parsing_is_exact() {
        for t in SentenceType::ALL {
            let name = t.as_str();
            for variant in [
                name.to_uppercase(),
                format!("{}{}", name[..1].to_uppercase(), &name[1..]),
                format!(" {name}"),
                format!("{name} "),
                format!("{name}s"),
            ] {
                assert!(variant.parse::<SentenceType>().is_err(), "{variant:?}");
            }
        }
        assert!("".parse::<SentenceType>().is_err());
    }

    #[test]
    fn the_error_reports_its_input_unchanged() {
        let error = " Imperative\n".parse::<SentenceType>().unwrap_err();
        assert_eq!(error.input(), " Imperative\n");
        assert_eq!(error.to_string(), r#"not a sentence type: " Imperative\n""#);
    }
}
