use std::fmt;

/// The class a part-of-speech tag falls into, as far as sentence analysis is
/// concerned.
///
/// This is the **entire** vocabulary the analyzer reacts to. Every rule in
/// [`analyze`] is stated in terms of these classes, so classifying a tag here
/// is enough to predict what the analyzer will do with the word carrying it.
///
/// # The tag set is Penn Treebank, and it is required
///
/// Tags are drawn from the Penn Treebank tag set (Santorini, *Part-of-Speech
/// Tagging Guidelines for the Penn Treebank Project*, 3rd revision, 1990;
/// Marcus, Santorini & Marcinkiewicz, *Building a Large Annotated Corpus of
/// English: The Penn Treebank*, Computational Linguistics 19(2), 1993). A tag
/// outside that set is [`TagClass::Other`]: it is carried through the analysis
/// untouched and matches no rule.
///
/// # Matching is whole-tag and byte-exact
///
/// [`TagClass::of`] compares the **whole** tag, byte for byte. It does not fold
/// case, trim whitespace, normalise, or search for a substring. `"nn"` is not
/// `"NN"`, `" NN"` is not `"NN"`, and — the case that matters most — a tag that
/// merely *contains* `NN` or `IN` is not a noun or a preposition:
///
/// ```
/// use verbora_analyzers::TagClass;
///
/// assert_eq!(TagClass::of("NN"), TagClass::Noun);
/// assert_eq!(TagClass::of("NNPS"), TagClass::Noun);
/// assert_eq!(TagClass::of("IN"), TagClass::Preposition);
///
/// // Ambiguity classes, lowercase tags and padded tags match nothing.
/// assert_eq!(TagClass::of("NN|IN"), TagClass::Other);
/// assert_eq!(TagClass::of("VBIN"), TagClass::Other);
/// assert_eq!(TagClass::of("nn"), TagClass::Other);
/// ```
///
/// Whole-tag matching is not a detail. A tagger's lexicon routinely carries
/// ambiguity classes such as `NN|IN`, `VBG|NN` and `WP|IN`; under substring
/// matching every one of them would open or close a prepositional phrase, and
/// `NN|IN` would open **and** close one on the same word.
///
/// [`analyze`]: crate::analyze
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum TagClass {
    /// `IN` — preposition or subordinating conjunction. Opens a prepositional
    /// phrase.
    ///
    /// `TO` is deliberately **not** here: Penn Treebank uses one tag for both
    /// the infinitive marker (*to vote*) and the preposition (*to the store*),
    /// so a `TO` word carries no evidence of which it is.
    Preposition,
    /// `NN`, `NNS`, `NNP`, `NNPS` — the four noun tags. Closes a prepositional
    /// phrase, inclusive of the noun itself: the noun is the head of the phrase's
    /// object, so it belongs inside the phrase.
    Noun,
    /// `VB` — verb, base form. The only tag that can head an English imperative
    /// clause, and therefore the only one that triggers implied-subject
    /// detection.
    BaseVerb,
    /// `MD`, `VBD`, `VBP`, `VBZ` — modal and tensed verb forms.
    ///
    /// A clause cannot *begin* with one of these unless its subject and
    /// operator are inverted, which is why a sentence-initial finite verb reads
    /// as a question, and why the operator of a tag question is one of these.
    FiniteVerb,
    /// `VBG`, `VBN` — gerund/present participle and past participle.
    ///
    /// Opens the predicate like any other verb, but carries no clause-type
    /// evidence: a participle-initial sentence is a participial adjunct
    /// (*Running late, he left*), not an inversion.
    NonfiniteVerb,
    /// `WDT`, `WP`, `WP$`, `WRB` — the wh-words.
    WhWord,
    /// `RB`, `RBR`, `RBS` — adverbs. May precede an imperative verb
    /// (*Always look both ways*) and may separate a tag question's operator from
    /// its pronoun (*isn't it*).
    Adverb,
    /// `UH` — interjection. May precede an imperative verb (*Please vote*).
    Interjection,
    /// `PRP` — personal pronoun; the pronoun half of a tag question.
    ///
    /// `PRP$` (possessive pronoun) is not here: it is a determiner in
    /// distribution and cannot close a tag question.
    Pronoun,
    /// Every other tag, in the Penn Treebank tag set or not. Matches no rule.
    Other,
}

impl TagClass {
    /// Every class, in declaration order.
    pub const ALL: [Self; 10] = [
        Self::Preposition,
        Self::Noun,
        Self::BaseVerb,
        Self::FiniteVerb,
        Self::NonfiniteVerb,
        Self::WhWord,
        Self::Adverb,
        Self::Interjection,
        Self::Pronoun,
        Self::Other,
    ];

    /// Classifies one Penn Treebank tag.
    ///
    /// Total: every input has a class, and an unrecognised tag is
    /// [`TagClass::Other`] rather than an error. Comparison is whole-tag and
    /// byte-exact — see the [type documentation](Self) for why.
    #[must_use]
    pub fn of(tag: &str) -> Self {
        match tag {
            "IN" => Self::Preposition,
            "NN" | "NNS" | "NNP" | "NNPS" => Self::Noun,
            "VB" => Self::BaseVerb,
            "MD" | "VBD" | "VBP" | "VBZ" => Self::FiniteVerb,
            "VBG" | "VBN" => Self::NonfiniteVerb,
            "WDT" | "WP" | "WP$" | "WRB" => Self::WhWord,
            "RB" | "RBR" | "RBS" => Self::Adverb,
            "UH" => Self::Interjection,
            "PRP" => Self::Pronoun,
            _ => Self::Other,
        }
    }

    /// Whether this class is a verb of any kind — [`Self::BaseVerb`],
    /// [`Self::FiniteVerb`] or [`Self::NonfiniteVerb`].
    ///
    /// This is the test that opens the predicate. All seven Penn verb tags
    /// (`MD`, `VB`, `VBD`, `VBG`, `VBN`, `VBP`, `VBZ`) satisfy it.
    ///
    /// ```
    /// use verbora_analyzers::TagClass;
    ///
    /// for tag in ["MD", "VB", "VBD", "VBG", "VBN", "VBP", "VBZ"] {
    ///     assert!(TagClass::of(tag).is_verb(), "{tag}");
    /// }
    /// assert!(!TagClass::of("NN").is_verb());
    /// ```
    #[must_use]
    pub const fn is_verb(self) -> bool {
        matches!(
            self,
            Self::BaseVerb | Self::FiniteVerb | Self::NonfiniteVerb
        )
    }

    /// Whether a word with this class may sit between the start of an
    /// imperative clause and its verb — [`Self::Adverb`] or
    /// [`Self::Interjection`].
    #[must_use]
    pub const fn precedes_imperative_verb(self) -> bool {
        matches!(self, Self::Adverb | Self::Interjection)
    }

    /// A short, stable name for the class. Not a Penn Treebank tag.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Preposition => "preposition",
            Self::Noun => "noun",
            Self::BaseVerb => "base verb",
            Self::FiniteVerb => "finite verb",
            Self::NonfiniteVerb => "nonfinite verb",
            Self::WhWord => "wh-word",
            Self::Adverb => "adverb",
            Self::Interjection => "interjection",
            Self::Pronoun => "pronoun",
            Self::Other => "other",
        }
    }
}

impl fmt::Display for TagClass {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The 36 word tags of the Penn Treebank tag set, in the order Santorini's
    /// guidelines list them, plus the punctuation tags the treebank uses.
    ///
    /// Transcribed from the published tag set, not from this crate's output.
    const PENN_WORD_TAGS: [&str; 36] = [
        "CC", "CD", "DT", "EX", "FW", "IN", "JJ", "JJR", "JJS", "LS", "MD", "NN", "NNS", "NNP",
        "NNPS", "PDT", "POS", "PRP", "PRP$", "RB", "RBR", "RBS", "RP", "SYM", "TO", "UH", "VB",
        "VBD", "VBG", "VBN", "VBP", "VBZ", "WDT", "WP", "WP$", "WRB",
    ];
    const PENN_PUNCT_TAGS: [&str; 9] = [".", ",", ":", "``", "''", "-LRB-", "-RRB-", "#", "$"];

    /// Walks **every** tag of the tag set this crate specifies and pins its
    /// class, then checks the totals. Sampling a few representatives is what
    /// let substring matching survive; this enumerates.
    #[test]
    fn every_penn_tag_is_classified_and_the_totals_add_up() {
        let mut counts = [0usize; TagClass::ALL.len()];
        for tag in PENN_WORD_TAGS.iter().chain(PENN_PUNCT_TAGS.iter()) {
            let class = TagClass::of(tag);
            let slot = TagClass::ALL
                .iter()
                .position(|c| *c == class)
                .expect("TagClass::ALL lists every variant");
            counts[slot] += 1;
        }

        // Expected counts, read off the Penn tag set by hand:
        //   preposition   IN                                    -> 1
        //   noun          NN NNS NNP NNPS                        -> 4
        //   base verb     VB                                     -> 1
        //   finite verb   MD VBD VBP VBZ                          -> 4
        //   nonfinite     VBG VBN                                -> 2
        //   wh-word       WDT WP WP$ WRB                          -> 4
        //   adverb        RB RBR RBS                              -> 3
        //   interjection  UH                                     -> 1
        //   pronoun       PRP                                    -> 1
        //   other         45 - 21                                -> 24
        assert_eq!(counts, [1, 4, 1, 4, 2, 4, 3, 1, 1, 24]);
        assert_eq!(counts.iter().sum::<usize>(), 45);
        assert_eq!(PENN_WORD_TAGS.len() + PENN_PUNCT_TAGS.len(), 45);
    }

    /// Every Penn tag maps to exactly one class, and re-classifying the same
    /// spelling is stable — the tag string is never rewritten on the way in.
    #[test]
    fn classification_is_a_function_of_the_exact_tag_bytes() {
        for tag in PENN_WORD_TAGS.iter().chain(PENN_PUNCT_TAGS.iter()) {
            let class = TagClass::of(tag);
            assert_eq!(
                TagClass::of(tag),
                class,
                "{tag} classified differently twice"
            );

            // A padded, lowercased or extended spelling is a different tag.
            let lower = tag.to_ascii_lowercase();
            if lower != *tag {
                assert_eq!(TagClass::of(&lower), TagClass::Other, "{lower}");
            }
            assert_eq!(TagClass::of(&format!(" {tag}")), TagClass::Other, "{tag}");
            assert_eq!(TagClass::of(&format!("{tag} ")), TagClass::Other, "{tag}");
        }
    }

    /// The ambiguity-class spellings a Brill lexicon carries. Every one of them
    /// contains `IN` or `NN` as a substring; none of them is a preposition or a
    /// noun. This is the exact shape that substring matching got wrong.
    #[test]
    fn ambiguity_class_spellings_match_nothing() {
        const AMBIGUOUS: [&str; 16] = [
            "NN|IN",
            "IN|JJ",
            "IN|RB",
            "IN|RP",
            "JJ|IN",
            "RB|IN",
            "RP|IN",
            "VB|IN",
            "WP|IN",
            "VBG|NN",
            "NN|JJ",
            "NNS|NN",
            "NNP|VBN",
            "RB|NN|JJ",
            "NN|VBG",
            "VBG|NN|JJ",
        ];
        for tag in AMBIGUOUS {
            assert!(
                tag.contains("IN") || tag.contains("NN"),
                "{tag} is not a substring hazard, so it does not belong in this test"
            );
            assert_eq!(TagClass::of(tag), TagClass::Other, "{tag}");
        }
    }

    #[test]
    fn verb_predicate_covers_all_seven_penn_verb_tags() {
        let verbs: Vec<&str> = PENN_WORD_TAGS
            .iter()
            .copied()
            .filter(|t| TagClass::of(t).is_verb())
            .collect();
        assert_eq!(verbs, ["MD", "VB", "VBD", "VBG", "VBN", "VBP", "VBZ"]);
    }

    #[test]
    fn empty_and_non_ascii_tags_are_other() {
        for tag in ["", " ", "\u{feff}", "日本語", "😀", "N\u{0301}N", "IN\u{0}"] {
            assert_eq!(TagClass::of(tag), TagClass::Other, "{tag:?}");
        }
    }

    #[test]
    fn names_are_unique_and_displayed() {
        let mut names: Vec<&str> = TagClass::ALL.iter().map(|c| c.name()).collect();
        names.sort_unstable();
        let before = names.len();
        names.dedup();
        assert_eq!(names.len(), before);
        assert_eq!(TagClass::Noun.to_string(), "noun");
    }
}
