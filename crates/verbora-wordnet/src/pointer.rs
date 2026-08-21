//! Relational pointers between synsets.

use std::fmt;
use std::num::NonZeroU8;

use crate::pos::{PartOfSpeech, SynsetType};
use crate::synset::SynsetOffset;

/// One of the twenty-six relations WordNet records between synsets.
///
/// The set is closed: `wninput(5WN)` enumerates it, and a `data.*` record
/// carrying anything else is malformed rather than carrying a relation this
/// crate does not know about. Reading one therefore fails with
/// [`RecordError::InvalidField`](crate::RecordError::InvalidField) instead of
/// producing a pointer nothing can interpret.
///
/// Several symbols are used by more than one part of speech, and one — `\` — has
/// two different meanings depending on the file it appears in, which is why
/// [`PointerSymbol::name`] takes the category as an argument.
///
/// ```
/// use verbora_wordnet::PointerSymbol;
///
/// assert_eq!(PointerSymbol::from_symbol("@"), Some(PointerSymbol::Hypernym));
/// assert_eq!(PointerSymbol::from_symbol("@i"), Some(PointerSymbol::InstanceHypernym));
/// assert_eq!(PointerSymbol::Hypernym.symbol(), "@");
/// assert_eq!(PointerSymbol::from_symbol("??"), None);
/// ```
/// Sealed, because this enum has already had to grow once: `wndb(5WN)`'s
/// symbol table is what a WordNet release ships, not what this crate decides,
/// and a release that adds a relation must not break every downstream `match`.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum PointerSymbol {
    /// `!` — antonym. The only lexical relation in the core noun set.
    Antonym,
    /// `@` — hypernym: a more general synset.
    Hypernym,
    /// `@i` — instance hypernym: the class a named individual belongs to.
    InstanceHypernym,
    /// `~` — hyponym: a more specific synset.
    Hyponym,
    /// `~i` — instance hyponym: a named individual of this class.
    InstanceHyponym,
    /// `#m` — member holonym: a whole this synset is a member of.
    MemberHolonym,
    /// `#s` — substance holonym: a whole this synset is a substance of.
    SubstanceHolonym,
    /// `#p` — part holonym: a whole this synset is a part of.
    PartHolonym,
    /// `%m` — member meronym: a member of this synset.
    MemberMeronym,
    /// `%s` — substance meronym: a substance of this synset.
    SubstanceMeronym,
    /// `%p` — part meronym: a part of this synset.
    PartMeronym,
    /// `=` — attribute: the noun/adjective pair "weight" ↔ "heavy".
    Attribute,
    /// `+` — derivationally related form.
    DerivationallyRelatedForm,
    /// `;c` — domain of synset, topic.
    DomainOfTopic,
    /// `-c` — member of this domain, topic.
    MemberOfTopic,
    /// `;r` — domain of synset, region.
    DomainOfRegion,
    /// `-r` — member of this domain, region.
    MemberOfRegion,
    /// `;u` — domain of synset, usage.
    DomainOfUsage,
    /// `-u` — member of this domain, usage.
    MemberOfUsage,
    /// `;` — a domain pointer with no class letter.
    ///
    /// Only an **index** file writes this. An index entry lists which relations
    /// the lemma's senses carry, and the topic/region/usage distinction is a
    /// property of the individual sense, so the class letter appears only in
    /// the data record. Measured over WordNet 3.1's shipped `index.*` files:
    /// 8,936 bare `;` and zero `;c`, against 4,109 `;c` in `data.noun` alone.
    ///
    /// Reading it as "one of [`DomainOfTopic`](Self::DomainOfTopic),
    /// [`DomainOfRegion`](Self::DomainOfRegion) or
    /// [`DomainOfUsage`](Self::DomainOfUsage), which one is in the data record"
    /// is what the format means. Collapsing it into any single one of them
    /// would state a class the index does not.
    Domain,
    /// `-` — a member-of-domain pointer with no class letter.
    ///
    /// The inverse of [`Domain`](Self::Domain), and index-only for the same
    /// reason. 1,295 occurrences in WordNet 3.1's `index.*`.
    Member,
    /// `*` — entailment (verbs).
    Entailment,
    /// `>` — cause (verbs).
    Cause,
    /// `^` — also see.
    AlsoSee,
    /// `$` — verb group.
    VerbGroup,
    /// `&` — similar to: links an adjective satellite to its head.
    SimilarTo,
    /// `<` — participle of verb (adjectives).
    ParticipleOfVerb,
    /// `\` — pertainym in `data.adj`, derived-from-adjective in `data.adv`.
    Pertainym,
}

impl PointerSymbol {
    /// All twenty-six, in the order `wninput(5WN)` tabulates them.
    pub const ALL: [Self; 28] = [
        Self::Antonym,
        Self::Hypernym,
        Self::InstanceHypernym,
        Self::Hyponym,
        Self::InstanceHyponym,
        Self::MemberHolonym,
        Self::SubstanceHolonym,
        Self::PartHolonym,
        Self::MemberMeronym,
        Self::SubstanceMeronym,
        Self::PartMeronym,
        Self::Attribute,
        Self::DerivationallyRelatedForm,
        Self::DomainOfTopic,
        Self::MemberOfTopic,
        Self::DomainOfRegion,
        Self::MemberOfRegion,
        Self::DomainOfUsage,
        Self::MemberOfUsage,
        Self::Domain,
        Self::Member,
        Self::Entailment,
        Self::Cause,
        Self::AlsoSee,
        Self::VerbGroup,
        Self::SimilarTo,
        Self::ParticipleOfVerb,
        Self::Pertainym,
    ];

    /// The relation a `ptr_symbol` field names, or `None` if it names none.
    ///
    /// Matching is exact and byte-for-byte: the two-character symbols (`@i`,
    /// `#p`, `;c` …) are distinct relations from their one-character prefixes.
    #[must_use]
    pub fn from_symbol(symbol: &str) -> Option<Self> {
        // A `match` over string literals compiles to a length-and-bytes decision
        // tree: no allocation, no hashing, no table to keep in sync.
        Some(match symbol {
            "!" => Self::Antonym,
            "@" => Self::Hypernym,
            "@i" => Self::InstanceHypernym,
            "~" => Self::Hyponym,
            "~i" => Self::InstanceHyponym,
            "#m" => Self::MemberHolonym,
            "#s" => Self::SubstanceHolonym,
            "#p" => Self::PartHolonym,
            "%m" => Self::MemberMeronym,
            "%s" => Self::SubstanceMeronym,
            "%p" => Self::PartMeronym,
            "=" => Self::Attribute,
            "+" => Self::DerivationallyRelatedForm,
            ";" => Self::Domain,
            "-" => Self::Member,
            ";c" => Self::DomainOfTopic,
            "-c" => Self::MemberOfTopic,
            ";r" => Self::DomainOfRegion,
            "-r" => Self::MemberOfRegion,
            ";u" => Self::DomainOfUsage,
            "-u" => Self::MemberOfUsage,
            "*" => Self::Entailment,
            ">" => Self::Cause,
            "^" => Self::AlsoSee,
            "$" => Self::VerbGroup,
            "&" => Self::SimilarTo,
            "<" => Self::ParticipleOfVerb,
            "\\" => Self::Pertainym,
            _ => return None,
        })
    }

    /// The symbol as written in the dictionary files.
    #[must_use]
    pub fn symbol(self) -> &'static str {
        match self {
            Self::Antonym => "!",
            Self::Hypernym => "@",
            Self::InstanceHypernym => "@i",
            Self::Hyponym => "~",
            Self::InstanceHyponym => "~i",
            Self::MemberHolonym => "#m",
            Self::SubstanceHolonym => "#s",
            Self::PartHolonym => "#p",
            Self::MemberMeronym => "%m",
            Self::SubstanceMeronym => "%s",
            Self::PartMeronym => "%p",
            Self::Attribute => "=",
            Self::DerivationallyRelatedForm => "+",
            Self::DomainOfTopic => ";c",
            Self::MemberOfTopic => "-c",
            Self::DomainOfRegion => ";r",
            Self::MemberOfRegion => "-r",
            Self::DomainOfUsage => ";u",
            Self::MemberOfUsage => "-u",
            Self::Domain => ";",
            Self::Member => "-",
            Self::Entailment => "*",
            Self::Cause => ">",
            Self::AlsoSee => "^",
            Self::VerbGroup => "$",
            Self::SimilarTo => "&",
            Self::ParticipleOfVerb => "<",
            Self::Pertainym => "\\",
        }
    }

    /// The relation's English name in the file `pos` belongs to.
    ///
    /// Only [`PointerSymbol::Pertainym`] depends on the argument: `wninput(5WN)`
    /// gives `\` as *pertainym (pertains to noun)* in `data.adj` and as *derived
    /// from adjective* in `data.adv`. Every other symbol answers the same name
    /// for all four categories.
    ///
    /// ```
    /// use verbora_wordnet::{PartOfSpeech, PointerSymbol};
    ///
    /// let p = PointerSymbol::Pertainym;
    /// assert_eq!(p.name(PartOfSpeech::Adjective), "pertainym (pertains to noun)");
    /// assert_eq!(p.name(PartOfSpeech::Adverb), "derived from adjective");
    /// ```
    #[must_use]
    pub fn name(self, pos: PartOfSpeech) -> &'static str {
        match self {
            Self::Antonym => "antonym",
            Self::Hypernym => "hypernym",
            Self::InstanceHypernym => "instance hypernym",
            Self::Hyponym => "hyponym",
            Self::InstanceHyponym => "instance hyponym",
            Self::MemberHolonym => "member holonym",
            Self::SubstanceHolonym => "substance holonym",
            Self::PartHolonym => "part holonym",
            Self::MemberMeronym => "member meronym",
            Self::SubstanceMeronym => "substance meronym",
            Self::PartMeronym => "part meronym",
            Self::Attribute => "attribute",
            Self::DerivationallyRelatedForm => "derivationally related form",
            Self::DomainOfTopic => "domain of synset - topic",
            Self::MemberOfTopic => "member of this domain - topic",
            Self::DomainOfRegion => "domain of synset - region",
            Self::MemberOfRegion => "member of this domain - region",
            Self::Domain => "domain of synset - class unstated",
            Self::Member => "member of this domain - class unstated",
            Self::DomainOfUsage => "domain of synset - usage",
            Self::MemberOfUsage => "member of this domain - usage",
            Self::Entailment => "entailment",
            Self::Cause => "cause",
            Self::AlsoSee => "also see",
            Self::VerbGroup => "verb group",
            Self::SimilarTo => "similar to",
            Self::ParticipleOfVerb => "participle of verb",
            Self::Pertainym => match pos {
                PartOfSpeech::Adverb => "derived from adjective",
                _ => "pertainym (pertains to noun)",
            },
        }
    }
}

impl fmt::Display for PointerSymbol {
    /// Writes the symbol as the dictionary files spell it.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.symbol())
    }
}

/// Whether a pointer relates two synsets or two individual words.
///
/// `wndb(5WN)`'s `source/target` field is four hexadecimal digits: the word
/// number within the source synset followed by the word number within the
/// target synset. `0000` — both halves zero — marks a *semantic* relation
/// between the synsets as wholes; anything else marks a *lexical* relation
/// between one word of each.
///
/// Modelling that as an enum rather than as a raw `u16` means the two readings
/// cannot be confused, and a half-zero field (which the format does not define)
/// is rejected at parse time instead of being silently rounded to one of them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PointerScope {
    /// `0000` — the relation holds between the two synsets as wholes.
    Semantic,
    /// The relation holds between one word of each synset. Word numbers are
    /// 1-based, as `wndb(5WN)` writes them.
    Lexical {
        /// Which word of the synset carrying this pointer.
        source_word: NonZeroU8,
        /// Which word of the target synset.
        target_word: NonZeroU8,
    },
}

/// A relational pointer from one synset to another.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Pointer {
    /// Which relation this is.
    pub symbol: PointerSymbol,
    /// Byte offset of the target synset within the file for
    /// [`Pointer::synset_type`]'s part of speech.
    pub offset: SynsetOffset,
    /// The target synset's own type, which selects the file to read it from.
    pub synset_type: SynsetType,
    /// Whether the relation is between synsets or between individual words.
    pub scope: PointerScope,
}

impl Pointer {
    /// The file pair the target lives in.
    #[must_use]
    pub fn part_of_speech(self) -> PartOfSpeech {
        self.synset_type.part_of_speech()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Walks the whole table rather than sampling it: every symbol must round
    /// trip, all twenty-eight spellings must be distinct, and nothing outside
    /// the table may parse.
    #[test]
    fn every_symbol_round_trips_and_the_table_is_injective() {
        let mut spellings: Vec<&str> = Vec::with_capacity(PointerSymbol::ALL.len());
        for s in PointerSymbol::ALL {
            assert_eq!(PointerSymbol::from_symbol(s.symbol()), Some(s), "{s:?}");
            assert_eq!(s.to_string(), s.symbol());
            spellings.push(s.symbol());
        }
        assert_eq!(spellings.len(), 28);
        spellings.sort_unstable();
        let before = spellings.len();
        spellings.dedup();
        assert_eq!(spellings.len(), before, "two variants share a spelling");
    }

    /// Enumerates every one- and two-character ASCII string — 128 + 16 384
    /// candidates — and checks that exactly the twenty-eight documented ones
    /// parse. A sampled test would not have caught a stray arm.
    #[test]
    fn exactly_twenty_eight_ascii_strings_parse_as_symbols() {
        let mut accepted = Vec::new();
        for a in 0u8..=127 {
            let one = (a as char).to_string();
            if PointerSymbol::from_symbol(&one).is_some() {
                accepted.push(one);
            }
            for b in 0u8..=127 {
                let two = format!("{}{}", a as char, b as char);
                if PointerSymbol::from_symbol(&two).is_some() {
                    accepted.push(two);
                }
            }
        }
        accepted.sort_unstable();
        let mut expected: Vec<String> = PointerSymbol::ALL
            .iter()
            .map(|s| s.symbol().to_owned())
            .collect();
        expected.sort_unstable();
        assert_eq!(accepted, expected);
        assert_eq!(PointerSymbol::from_symbol(""), None);
        assert_eq!(PointerSymbol::from_symbol("@@"), None);
        assert_eq!(PointerSymbol::from_symbol("😀"), None);
    }

    #[test]
    fn every_symbol_has_a_name_in_every_category() {
        for s in PointerSymbol::ALL {
            for pos in PartOfSpeech::ALL {
                assert!(!s.name(pos).is_empty(), "{s:?} in {pos:?}");
            }
        }
        // The one symbol whose name depends on the file it was read from.
        let p = PointerSymbol::Pertainym;
        assert_ne!(
            p.name(PartOfSpeech::Adjective),
            p.name(PartOfSpeech::Adverb)
        );
        for s in PointerSymbol::ALL {
            if s == PointerSymbol::Pertainym {
                continue;
            }
            for pos in PartOfSpeech::ALL {
                assert_eq!(s.name(pos), s.name(PartOfSpeech::Noun), "{s:?}");
            }
        }
    }
}
