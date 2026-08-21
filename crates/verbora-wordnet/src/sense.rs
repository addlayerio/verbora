//! Sense addressing: `lemma#pos#number`.

use std::fmt;
use std::num::NonZeroU16;
use std::str::FromStr;

use crate::pos::{PartOfSpeech, SynsetType};

/// Which sense of a lemma, counting from one.
///
/// `wndb(5WN)` numbers the senses of a lemma in the order its index line lists
/// their offsets, most-frequently-tagged first, starting at 1. Sense zero does
/// not exist, so this type cannot represent it: the invalid state is
/// unrepresentable rather than checked at every use.
///
/// ```
/// use verbora_wordnet::SenseNumber;
///
/// assert_eq!(SenseNumber::FIRST.as_usize(), 1);
/// assert_eq!(SenseNumber::from_u16(3).unwrap().to_string(), "3");
/// assert_eq!(SenseNumber::from_u16(0), None);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct SenseNumber(NonZeroU16);

impl SenseNumber {
    /// Sense 1 — the sense WordNet lists first.
    pub const FIRST: Self = Self(NonZeroU16::MIN);

    /// Wraps a number already known to be non-zero.
    #[must_use]
    pub const fn new(number: NonZeroU16) -> Self {
        Self(number)
    }

    /// The sense numbered `number`, or `None` for zero.
    #[must_use]
    pub const fn from_u16(number: u16) -> Option<Self> {
        match NonZeroU16::new(number) {
            Some(n) => Some(Self(n)),
            None => None,
        }
    }

    /// The number itself.
    #[must_use]
    pub const fn get(self) -> NonZeroU16 {
        self.0
    }

    /// The number as a `usize`, for indexing a sense list after subtracting one.
    #[must_use]
    pub const fn as_usize(self) -> usize {
        self.0.get() as usize
    }
}

impl fmt::Display for SenseNumber {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<NonZeroU16> for SenseNumber {
    fn from(number: NonZeroU16) -> Self {
        Self(number)
    }
}

/// A reference to exactly one sense of one word: `lemma#pos#number`.
///
/// The `lemma#pos#number` spelling is WordNet's own, as used by the `wn`
/// command-line browser. All three parts are required here: a `Sense` denotes
/// one sense, and "every sense of a word" is a different question, answered by
/// [`WordNet::senses`](crate::WordNet::senses).
///
/// The lemma is stored **exactly as written**. No case folding, no whitespace
/// rewriting: `Display` therefore round-trips whatever was parsed.
/// [`WordNet::sense`](crate::WordNet::sense) applies [`index_key`](crate::index_key)
/// when it goes to the file, which is the one place the transform belongs.
///
/// ```
/// use verbora_wordnet::{PartOfSpeech, Sense, SenseNumber};
///
/// let s: Sense = "entity#n#1".parse().unwrap();
/// assert_eq!(s.lemma, "entity");
/// assert_eq!(s.pos, PartOfSpeech::Noun);
/// assert_eq!(s.number, SenseNumber::FIRST);
/// assert_eq!(s.to_string(), "entity#n#1");
///
/// // `s` is the adjective-satellite tag, and satellites live in the adjective
/// // files, so it maps to the adjective category.
/// assert_eq!("cold#s#2".parse::<Sense>().unwrap().pos, PartOfSpeech::Adjective);
///
/// // The lemma is kept verbatim; normalisation happens at lookup time.
/// assert_eq!("New York#n#1".parse::<Sense>().unwrap().to_string(), "New York#n#1");
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Sense {
    /// The word, exactly as supplied.
    pub lemma: String,
    /// Which part of speech to look in.
    pub pos: PartOfSpeech,
    /// Which sense, counting from one.
    pub number: SenseNumber,
}

impl Sense {
    /// Builds a sense reference.
    #[must_use]
    pub fn new(lemma: impl Into<String>, pos: PartOfSpeech, number: SenseNumber) -> Self {
        Self {
            lemma: lemma.into(),
            pos,
            number,
        }
    }
}

impl fmt::Display for Sense {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}#{}#{}", self.lemma, self.pos.tag(), self.number)
    }
}

/// Why a `lemma#pos#number` string could not be parsed.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum ParseSenseError {
    /// The string did not have exactly three `#`-separated parts.
    Shape {
        /// How many parts were found.
        parts: usize,
    },
    /// The lemma part was empty.
    EmptyLemma,
    /// The category part was not one of `n`, `v`, `a`, `s`, `r`.
    UnknownPartOfSpeech(String),
    /// The sense number was not a positive integer.
    InvalidSenseNumber(String),
}

impl fmt::Display for ParseSenseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Shape { parts } => write!(
                f,
                "expected lemma#pos#number, found {parts} '#'-separated part(s)"
            ),
            Self::EmptyLemma => f.write_str("the lemma is empty"),
            Self::UnknownPartOfSpeech(tag) => {
                write!(f, "{tag:?} is not one of n, v, a, s, r")
            }
            Self::InvalidSenseNumber(n) => {
                write!(f, "{n:?} is not a sense number (they start at 1)")
            }
        }
    }
}

impl std::error::Error for ParseSenseError {}

impl FromStr for Sense {
    type Err = ParseSenseError;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        let parts: Vec<&str> = s.split('#').collect();
        let [lemma, tag, number] = parts.as_slice() else {
            return Err(ParseSenseError::Shape { parts: parts.len() });
        };
        if lemma.is_empty() {
            return Err(ParseSenseError::EmptyLemma);
        }
        let pos = SynsetType::from_tag(tag)
            .map(SynsetType::part_of_speech)
            .ok_or_else(|| ParseSenseError::UnknownPartOfSpeech((*tag).to_owned()))?;
        let number = number
            .parse::<u16>()
            .ok()
            .and_then(SenseNumber::from_u16)
            .ok_or_else(|| ParseSenseError::InvalidSenseNumber((*number).to_owned()))?;
        Ok(Self {
            lemma: (*lemma).to_owned(),
            pos,
            number,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_the_three_part_spelling() {
        assert_eq!(
            "entity#n#1".parse::<Sense>().unwrap(),
            Sense::new("entity", PartOfSpeech::Noun, SenseNumber::FIRST)
        );
        assert_eq!(
            "run#v#57".parse::<Sense>().unwrap().number,
            SenseNumber::from_u16(57).unwrap()
        );
        // Every documented tag maps in, including the satellite.
        for (tag, want) in [
            ("n", PartOfSpeech::Noun),
            ("v", PartOfSpeech::Verb),
            ("a", PartOfSpeech::Adjective),
            ("s", PartOfSpeech::Adjective),
            ("r", PartOfSpeech::Adverb),
        ] {
            let spec = format!("x#{tag}#1");
            assert_eq!(spec.parse::<Sense>().unwrap().pos, want, "{spec}");
        }
    }

    #[test]
    fn rejects_every_malformed_shape_with_a_named_reason() {
        let cases: [(&str, ParseSenseError); 8] = [
            ("", ParseSenseError::Shape { parts: 1 }),
            ("entity", ParseSenseError::Shape { parts: 1 }),
            ("entity#n", ParseSenseError::Shape { parts: 2 }),
            ("entity#n#1#2", ParseSenseError::Shape { parts: 4 }),
            ("#n#1", ParseSenseError::EmptyLemma),
            (
                "entity#z#1",
                ParseSenseError::UnknownPartOfSpeech("z".to_owned()),
            ),
            (
                "entity#n#0",
                ParseSenseError::InvalidSenseNumber("0".to_owned()),
            ),
            (
                "entity#n#x",
                ParseSenseError::InvalidSenseNumber("x".to_owned()),
            ),
        ];
        for (spec, want) in cases {
            assert_eq!(spec.parse::<Sense>().unwrap_err(), want, "{spec:?}");
        }
        // Case matters, as it does for every tag in the format.
        assert!("entity#N#1".parse::<Sense>().is_err());
        // A number too large for the field is refused rather than wrapped.
        assert!("entity#n#65536".parse::<Sense>().is_err());
        assert!("entity#n#-1".parse::<Sense>().is_err());
    }

    #[test]
    fn display_round_trips_exactly_what_was_parsed() {
        for spec in [
            "entity#n#1",
            "run#v#12",
            "fast#a#3",
            "quickly#r#1",
            "New York#n#1",
            "CAFÉ#n#2",
            "日本語#n#1",
        ] {
            assert_eq!(spec.parse::<Sense>().unwrap().to_string(), spec, "{spec}");
        }
        // The satellite tag maps into the adjective category, so it is the one
        // spelling that does not survive a round trip — by design, because `s`
        // names a synset type and `Sense` names a file.
        assert_eq!("cold#s#2".parse::<Sense>().unwrap().to_string(), "cold#a#2");
    }

    #[test]
    fn sense_numbers_start_at_one_and_cannot_be_zero() {
        assert_eq!(SenseNumber::FIRST.as_usize(), 1);
        assert_eq!(SenseNumber::from_u16(0), None);
        for n in 1u16..=1000 {
            let s = SenseNumber::from_u16(n).unwrap();
            assert_eq!(s.as_usize(), usize::from(n));
            assert_eq!(s.to_string(), n.to_string());
        }
    }
}
