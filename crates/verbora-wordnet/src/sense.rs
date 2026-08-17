//! Sense addressing — `find_sense` and `query_sense`.
//!
//! # These have no reference counterpart
//!
//! The reference `wordnet` module exports exactly one thing, the `WordNet` constructor,
//! and its nine methods are `get`, `lookup`, `lookupFromFiles`, `pushResults`,
//! `loadResultSynonyms`, `loadSynonyms`, `lookupSynonyms`, `getSynonyms` and
//! `getDataFile`. There is no `findSense` and no `querySense` anywhere in the
//! reference tree — verified by an exhaustive grep across the whole reference
//! tree, including its type declarations and its own specs.
//!
//! The two functions here are therefore a `verbora` **extension**, built on
//! the parity surface rather than beside it, and they are not covered by the
//! recorded fixtures. They are provided because a sense-addressed lookup is what
//! most callers actually want from a `word#pos#n` string, and because building
//! it out of [`WordNet::lookup_iter`] would otherwise invite everyone to
//! rediscover the sense-numbering subtlety below.
//!
//! # Sense numbers run forwards
//!
//! [`crate::WordNet::lookup`] returns synsets in the reference's `pop()`-driven
//! **reverse** order. Sense numbers do not follow that: sense 1 is the *first*
//! offset on the index line, which is the *last* record `lookup` yields for that
//! part of speech. Numbering the results as they arrive would number every word
//! backwards.

use std::fmt;
use std::str::FromStr;

use crate::data_file::DataRecord;
use crate::error::{Error, Result};
use crate::whitespace::normalize_lookup_word;
use crate::wordnet::{Pos, WordNet};

/// A reference to one sense of one word: `lemma#pos#number`.
///
/// The number is 1-based and optional; `entity#n` denotes every sense.
///
/// ```
/// use verbora_wordnet::{Pos, Sense};
///
/// let s: Sense = "entity#n#1".parse().unwrap();
/// assert_eq!(s.lemma, "entity");
/// assert_eq!(s.pos, Pos::Noun);
/// assert_eq!(s.number, Some(1));
/// assert_eq!(s.to_string(), "entity#n#1");
///
/// // Whitespace is normalised the same way lookup normalises it.
/// let s: Sense = "New York#n".parse().unwrap();
/// assert_eq!(s.lemma, "new_york");
/// assert_eq!(s.number, None);
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sense {
    /// The lemma, already lowercased with whitespace runs turned into `_`.
    pub lemma: String,
    /// Which part of speech to search.
    pub pos: Pos,
    /// 1-based sense number, or `None` for "all senses".
    pub number: Option<usize>,
}

impl Sense {
    /// Builds a sense reference, normalising `lemma` as `lookup` would.
    #[must_use]
    pub fn new(lemma: &str, pos: Pos, number: Option<usize>) -> Self {
        Self {
            lemma: normalize_lookup_word(lemma),
            pos,
            number,
        }
    }
}

impl fmt::Display for Sense {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}#{}", self.lemma, self.pos.tag())?;
        if let Some(n) = self.number {
            write!(f, "#{n}")?;
        }
        Ok(())
    }
}

/// Why a sense string could not be parsed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseSenseError(String);

impl fmt::Display for ParseSenseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for ParseSenseError {}

impl FromStr for Sense {
    type Err = ParseSenseError;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        let mut parts = s.split('#');
        let lemma = parts
            .next()
            .filter(|l| !l.is_empty())
            .ok_or_else(|| ParseSenseError(format!("{s:?}: empty lemma")))?;
        let tag = parts
            .next()
            .ok_or_else(|| ParseSenseError(format!("{s:?}: expected lemma#pos[#n]")))?;
        let pos = Pos::from_tag(tag).ok_or_else(|| {
            ParseSenseError(format!("{s:?}: {tag:?} is not one of n, v, a, s, r"))
        })?;
        let number =
            match parts.next() {
                None => None,
                Some(n) => Some(n.parse::<usize>().ok().filter(|&n| n >= 1).ok_or_else(|| {
                    ParseSenseError(format!("{s:?}: {n:?} is not a sense number"))
                })?),
            };
        if parts.next().is_some() {
            return Err(ParseSenseError(format!("{s:?}: too many '#' separators")));
        }
        Ok(Self {
            lemma: normalize_lookup_word(lemma),
            pos,
            number,
        })
    }
}

impl WordNet {
    /// Every sense of a lemma in one part of speech, numbered from 1 in **index
    /// file order**.
    ///
    /// `spec` is `lemma#pos` or `lemma#pos#n`; a sense number narrows the answer
    /// to that one sense (or none, if the word has fewer). Returns an empty
    /// vector when the lemma is not found — including when it is present but the
    /// reference's incomplete bisection cannot find it, since this is built on
    /// that same search.
    ///
    /// # Errors
    ///
    /// [`ParseSenseError`] wrapped as [`Error::UnknownPos`] for an unparsable
    /// spec, or any [`Error`] from reading the index file.
    ///
    /// ```no_run
    /// # fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// use verbora_wordnet::WordNet;
    ///
    /// let wn = WordNet::from_env()?;
    /// for sense in wn.query_sense("entity#n")? {
    ///     println!("{sense}");   // entity#n#1, entity#n#2, …
    /// }
    /// # Ok(()) }
    /// ```
    pub fn query_sense(&self, spec: &str) -> Result<Vec<Sense>> {
        let sense: Sense = spec
            .parse()
            .map_err(|e: ParseSenseError| Error::UnknownPos(e.to_string()))?;
        let Some(record) = self.index_file(sense.pos).lookup(&sense.lemma)? else {
            return Ok(Vec::new());
        };
        let count = record.synset_offset.len();
        let range: Vec<usize> = match sense.number {
            Some(n) if n <= count => vec![n],
            Some(_) => Vec::new(),
            None => (1..=count).collect(),
        };
        Ok(range
            .into_iter()
            .map(|n| Sense {
                lemma: sense.lemma.clone(),
                pos: sense.pos,
                number: Some(n),
            })
            .collect())
    }

    /// The synset for one numbered sense.
    ///
    /// `spec` must carry a sense number (`entity#n#1`); without one, sense 1 is
    /// assumed. Returns `None` when the lemma is not found or has fewer senses.
    ///
    /// # Errors
    ///
    /// As [`WordNet::query_sense`], plus anything reading the data file can
    /// return.
    ///
    /// ```no_run
    /// # fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// use verbora_wordnet::WordNet;
    ///
    /// let wn = WordNet::from_env()?;
    /// if let Some(synset) = wn.find_sense("entity#n#1")? {
    ///     println!("{}", synset.def);
    /// }
    /// # Ok(()) }
    /// ```
    pub fn find_sense(&self, spec: &str) -> Result<Option<DataRecord>> {
        let sense: Sense = spec
            .parse()
            .map_err(|e: ParseSenseError| Error::UnknownPos(e.to_string()))?;
        let Some(record) = self.index_file(sense.pos).lookup(&sense.lemma)? else {
            return Ok(None);
        };
        let n = sense.number.unwrap_or(1);
        // Sense numbers are 1-based over the index line's own order, which is the
        // REVERSE of the order `lookup` yields. Indexing forwards here is the
        // whole point of this function existing.
        let Some(&offset) = record.synset_offset.get(n - 1) else {
            return Ok(None);
        };
        Ok(Some(self.get_at(offset, sense.pos)?))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_the_spec_forms() {
        assert_eq!(
            "entity#n#1".parse::<Sense>().unwrap(),
            Sense::new("entity", Pos::Noun, Some(1))
        );
        assert_eq!(
            "entity#n".parse::<Sense>().unwrap(),
            Sense::new("entity", Pos::Noun, None)
        );
        // Satellite adjectives route to the adjective files, as everywhere else.
        assert_eq!("x#s".parse::<Sense>().unwrap().pos, Pos::Adj);
        // The lemma is normalised exactly as `lookup` normalises it.
        assert_eq!("New  York#n".parse::<Sense>().unwrap().lemma, "new_york");
        assert_eq!("CAFÉ#n".parse::<Sense>().unwrap().lemma, "café");
    }

    #[test]
    fn rejects_malformed_specs() {
        for bad in [
            "",
            "entity",
            "entity#",
            "entity#z",
            "entity#n#0",
            "entity#n#x",
            "entity#n#1#2",
            "#n#1",
            "entity#N",
        ] {
            assert!(bad.parse::<Sense>().is_err(), "{bad:?} should not parse");
        }
    }

    #[test]
    fn round_trips_through_display() {
        for s in ["entity#n#1", "run#v#12", "fast#a#3", "quickly#r#1"] {
            assert_eq!(s.parse::<Sense>().unwrap().to_string(), s);
        }
        assert_eq!("entity#n".parse::<Sense>().unwrap().to_string(), "entity#n");
    }
}
