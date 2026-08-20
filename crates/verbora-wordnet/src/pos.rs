//! Syntactic categories.

use std::fmt;

/// The four syntactic categories WordNet keeps a file pair for.
///
/// A dictionary is eight files — `index.noun`/`data.noun` and three siblings —
/// so this enum is simultaneously the part-of-speech tag written in an index
/// line's `pos` field and the routing key that selects a file pair.
///
/// Adjective satellites are **not** a fifth category: `wndb(5WN)` gives
/// `index.adj` and `data.adj` both the head adjectives and the satellites, and
/// distinguishes them only by a synset's own `ss_type`. That distinction lives
/// on [`SynsetType`], which is a different question from "which file".
///
/// ```
/// use verbora_wordnet::PartOfSpeech;
///
/// assert_eq!(PartOfSpeech::from_tag("n"), Some(PartOfSpeech::Noun));
/// assert_eq!(PartOfSpeech::Adjective.file_suffix(), "adj");
/// assert_eq!(PartOfSpeech::Adverb.tag(), "r");
///
/// // `s` is a synset type, not a file: it maps in through `SynsetType`.
/// assert_eq!(PartOfSpeech::from_tag("s"), None);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum PartOfSpeech {
    /// `n` — `index.noun` and `data.noun`.
    Noun,
    /// `v` — `index.verb` and `data.verb`.
    Verb,
    /// `a` — `index.adj` and `data.adj`, head adjectives and satellites alike.
    Adjective,
    /// `r` — `index.adv` and `data.adv`.
    Adverb,
}

impl PartOfSpeech {
    /// All four, in the order a dictionary's files are conventionally listed
    /// and the order [`WordNet::lookup`](crate::WordNet::lookup) consults them.
    pub const ALL: [Self; 4] = [Self::Noun, Self::Verb, Self::Adjective, Self::Adverb];

    /// The category an index file's `pos` field names, or `None` for anything
    /// else.
    ///
    /// `wndb(5WN)` restricts this field to `n`, `v`, `a` and `r`. Matching is
    /// exact: `"N"` and `"noun"` are not tags.
    #[must_use]
    pub fn from_tag(tag: &str) -> Option<Self> {
        match tag {
            "n" => Some(Self::Noun),
            "v" => Some(Self::Verb),
            "a" => Some(Self::Adjective),
            "r" => Some(Self::Adverb),
            _ => None,
        }
    }

    /// The tag as written in an index line: `n`, `v`, `a`, `r`.
    #[must_use]
    pub fn tag(self) -> &'static str {
        match self {
            Self::Noun => "n",
            Self::Verb => "v",
            Self::Adjective => "a",
            Self::Adverb => "r",
        }
    }

    /// The file-name suffix: `noun`, `verb`, `adj`, `adv`.
    #[must_use]
    pub fn file_suffix(self) -> &'static str {
        match self {
            Self::Noun => "noun",
            Self::Verb => "verb",
            Self::Adjective => "adj",
            Self::Adverb => "adv",
        }
    }

    /// The English name of the category.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::Noun => "noun",
            Self::Verb => "verb",
            Self::Adjective => "adjective",
            Self::Adverb => "adverb",
        }
    }
}

impl fmt::Display for PartOfSpeech {
    /// Writes the one-letter tag, as an index line does.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.tag())
    }
}

/// A synset's `ss_type`: the four categories plus the adjective satellite.
///
/// `wndb(5WN)` defines `ss_type` over `n`, `v`, `a`, `s` and `r`, where `s`
/// marks an adjective satellite — a synset in `data.adj` clustered under a head
/// adjective by a `&` (similar to) pointer. Satellites live in the adjective
/// files, which is what [`SynsetType::part_of_speech`] expresses.
///
/// ```
/// use verbora_wordnet::{PartOfSpeech, SynsetType};
///
/// let satellite = SynsetType::from_tag("s").unwrap();
/// assert_eq!(satellite, SynsetType::AdjectiveSatellite);
/// assert_eq!(satellite.part_of_speech(), PartOfSpeech::Adjective);
/// assert_eq!(satellite.tag(), "s");
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum SynsetType {
    /// `n` — a noun synset.
    Noun,
    /// `v` — a verb synset.
    Verb,
    /// `a` — a head adjective synset.
    Adjective,
    /// `s` — an adjective satellite synset.
    AdjectiveSatellite,
    /// `r` — an adverb synset.
    Adverb,
}

impl SynsetType {
    /// All five, in the order `wndb(5WN)` lists them.
    pub const ALL: [Self; 5] = [
        Self::Noun,
        Self::Verb,
        Self::Adjective,
        Self::AdjectiveSatellite,
        Self::Adverb,
    ];

    /// The type an `ss_type` field names, or `None` for anything else.
    #[must_use]
    pub fn from_tag(tag: &str) -> Option<Self> {
        match tag {
            "n" => Some(Self::Noun),
            "v" => Some(Self::Verb),
            "a" => Some(Self::Adjective),
            "s" => Some(Self::AdjectiveSatellite),
            "r" => Some(Self::Adverb),
            _ => None,
        }
    }

    /// The tag as written in a data record: `n`, `v`, `a`, `s`, `r`.
    #[must_use]
    pub fn tag(self) -> &'static str {
        match self {
            Self::Noun => "n",
            Self::Verb => "v",
            Self::Adjective => "a",
            Self::AdjectiveSatellite => "s",
            Self::Adverb => "r",
        }
    }

    /// Which file pair a synset of this type lives in.
    ///
    /// Satellites answer [`PartOfSpeech::Adjective`]; every other type maps to
    /// the category of the same name.
    #[must_use]
    pub fn part_of_speech(self) -> PartOfSpeech {
        match self {
            Self::Noun => PartOfSpeech::Noun,
            Self::Verb => PartOfSpeech::Verb,
            Self::Adjective | Self::AdjectiveSatellite => PartOfSpeech::Adjective,
            Self::Adverb => PartOfSpeech::Adverb,
        }
    }
}

impl fmt::Display for SynsetType {
    /// Writes the one-letter tag, as a data record does.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.tag())
    }
}

impl From<PartOfSpeech> for SynsetType {
    /// The head type of a category. [`PartOfSpeech::Adjective`] becomes
    /// [`SynsetType::Adjective`], never the satellite.
    fn from(pos: PartOfSpeech) -> Self {
        match pos {
            PartOfSpeech::Noun => Self::Noun,
            PartOfSpeech::Verb => Self::Verb,
            PartOfSpeech::Adjective => Self::Adjective,
            PartOfSpeech::Adverb => Self::Adverb,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Enumerates every tag `wndb(5WN)` defines for each field, plus a
    /// systematic sweep of the ones it does not, rather than sampling.
    #[test]
    fn index_pos_accepts_exactly_the_four_documented_tags() {
        assert_eq!(PartOfSpeech::from_tag("n"), Some(PartOfSpeech::Noun));
        assert_eq!(PartOfSpeech::from_tag("v"), Some(PartOfSpeech::Verb));
        assert_eq!(PartOfSpeech::from_tag("a"), Some(PartOfSpeech::Adjective));
        assert_eq!(PartOfSpeech::from_tag("r"), Some(PartOfSpeech::Adverb));

        // Every other single ASCII character, plus a handful of longer strings.
        for b in 0u8..=127 {
            let s = (b as char).to_string();
            if matches!(s.as_str(), "n" | "v" | "a" | "r") {
                continue;
            }
            assert_eq!(PartOfSpeech::from_tag(&s), None, "{s:?}");
        }
        for bad in ["", "s", "noun", "nn", " n", "N", "ن", "😀"] {
            assert_eq!(PartOfSpeech::from_tag(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn synset_type_accepts_exactly_the_five_documented_tags() {
        for t in SynsetType::ALL {
            assert_eq!(SynsetType::from_tag(t.tag()), Some(t));
        }
        for b in 0u8..=127 {
            let s = (b as char).to_string();
            if matches!(s.as_str(), "n" | "v" | "a" | "s" | "r") {
                continue;
            }
            assert_eq!(SynsetType::from_tag(&s), None, "{s:?}");
        }
    }

    #[test]
    fn every_tag_round_trips_and_every_type_routes_to_a_file() {
        for p in PartOfSpeech::ALL {
            assert_eq!(PartOfSpeech::from_tag(p.tag()), Some(p));
            assert_eq!(SynsetType::from(p).part_of_speech(), p);
            assert_eq!(p.to_string(), p.tag());
        }
        for t in SynsetType::ALL {
            assert_eq!(SynsetType::from_tag(t.tag()), Some(t));
            assert_eq!(t.to_string(), t.tag());
        }
        // The satellite is the one type whose file is not named after it.
        assert_eq!(
            SynsetType::AdjectiveSatellite.part_of_speech(),
            PartOfSpeech::Adjective
        );
        assert_eq!(
            SynsetType::from(PartOfSpeech::Adjective),
            SynsetType::Adjective
        );
    }

    #[test]
    fn the_four_file_suffixes_are_distinct() {
        let mut seen: Vec<&str> = PartOfSpeech::ALL.iter().map(|p| p.file_suffix()).collect();
        seen.sort_unstable();
        assert_eq!(seen, ["adj", "adv", "noun", "verb"]);
    }
}
