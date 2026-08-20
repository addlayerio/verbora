use std::fmt;

/// The kind of mark that ends a sentence.
///
/// Terminal punctuation is the strongest sentence-type signal available to a
/// rule-based analyzer, so [`analyze`] looks for it first and lets it decide
/// the classification whenever it is present.
///
/// # The recognised set is closed, and stated here
///
/// This is an explicit Verbora specification, not a derivation from a Unicode
/// character property. A token is a terminator when it is **exactly one**
/// Unicode scalar value and that scalar appears in one of the three tables
/// below; every other token is not a terminator, whatever its Unicode
/// properties. Each entry is identified by its Unicode character name.
///
/// Every scalar listed has `Sentence_Terminal=Yes` in the Unicode Character
/// Database, or belongs to the `ATerm` set of UAX #29 (*Unicode Text
/// Segmentation*) — the four full-stop scalars U+002E, U+2024, U+FE52 and
/// U+FF0E. Scalars that carry `Sentence_Terminal=Yes` but whose Unicode name
/// identifies them as something other than a full stop, question mark or
/// exclamation mark (U+01C3 LATIN LETTER RETROFLEX CLICK, for instance) are
/// excluded, because their sentence-final use cannot be assumed from the
/// character alone.
///
/// ## [`Terminator::FullStop`]
///
/// | Scalar | Name |
/// |---|---|
/// | U+002E | FULL STOP |
/// | U+0589 | ARMENIAN FULL STOP |
/// | U+06D4 | ARABIC FULL STOP |
/// | U+0964 | DEVANAGARI DANDA |
/// | U+2024 | ONE DOT LEADER |
/// | U+3002 | IDEOGRAPHIC FULL STOP |
/// | U+FE52 | SMALL FULL STOP |
/// | U+FF0E | FULLWIDTH FULL STOP |
/// | U+FF61 | HALFWIDTH IDEOGRAPHIC FULL STOP |
///
/// ## [`Terminator::Question`]
///
/// | Scalar | Name |
/// |---|---|
/// | U+003F | QUESTION MARK |
/// | U+061F | ARABIC QUESTION MARK |
/// | U+203D | INTERROBANG |
/// | U+2047 | DOUBLE QUESTION MARK |
/// | U+2048 | QUESTION EXCLAMATION MARK |
/// | U+2049 | EXCLAMATION QUESTION MARK |
/// | U+2E2E | REVERSED QUESTION MARK |
/// | U+FE56 | SMALL QUESTION MARK |
/// | U+FF1F | FULLWIDTH QUESTION MARK |
///
/// The three marks that combine both functions — U+203D, U+2048 and U+2049 —
/// are questions: an emphatic question is still a question, whereas treating
/// them as exclamations would lose the interrogative reading entirely.
///
/// ## [`Terminator::Exclamation`]
///
/// | Scalar | Name |
/// |---|---|
/// | U+0021 | EXCLAMATION MARK |
/// | U+055C | ARMENIAN EXCLAMATION MARK |
/// | U+07F9 | NKO EXCLAMATION MARK |
/// | U+203C | DOUBLE EXCLAMATION MARK |
/// | U+FE57 | SMALL EXCLAMATION MARK |
/// | U+FF01 | FULLWIDTH EXCLAMATION MARK |
///
/// # What is deliberately not a terminator
///
/// * **Multi-scalar tokens.** `"..."`, `"?!"` and `"!!"` are three, two and two
///   scalars, so none is recognised. A tokenizer that emits them as single
///   tokens should map them itself and call [`analyze_with_terminator`].
/// * **U+2026 HORIZONTAL ELLIPSIS.** It is not `Sentence_Terminal`, and it ends
///   a sentence only sometimes.
/// * **Opening marks.** U+00BF INVERTED QUESTION MARK and U+00A1 INVERTED
///   EXCLAMATION MARK open a Spanish sentence rather than ending one.
/// * **The empty token.** Zero scalars is not one scalar.
///
/// ```
/// use verbora_analyzers::Terminator;
///
/// assert_eq!(Terminator::from_token("?"), Some(Terminator::Question));
/// assert_eq!(Terminator::from_token("！"), Some(Terminator::Exclamation));
/// assert_eq!(Terminator::from_token("。"), Some(Terminator::FullStop));
///
/// assert_eq!(Terminator::from_token("?!"), None);
/// assert_eq!(Terminator::from_token("…"), None);
/// assert_eq!(Terminator::from_token(""), None);
/// ```
///
/// [`analyze`]: crate::analyze
/// [`analyze_with_terminator`]: crate::analyze_with_terminator
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Terminator {
    /// A full stop. Leaves the clause type to the clause itself.
    FullStop,
    /// A question mark. Classifies the sentence as interrogative outright.
    Question,
    /// An exclamation mark. Classifies the sentence as exclamative, unless the
    /// clause is imperative.
    Exclamation,
}

const FULL_STOPS: [char; 9] = [
    '\u{002E}', '\u{0589}', '\u{06D4}', '\u{0964}', '\u{2024}', '\u{3002}', '\u{FE52}', '\u{FF0E}',
    '\u{FF61}',
];
const QUESTIONS: [char; 9] = [
    '\u{003F}', '\u{061F}', '\u{203D}', '\u{2047}', '\u{2048}', '\u{2049}', '\u{2E2E}', '\u{FE56}',
    '\u{FF1F}',
];
const EXCLAMATIONS: [char; 6] = [
    '\u{0021}', '\u{055C}', '\u{07F9}', '\u{203C}', '\u{FE57}', '\u{FF01}',
];

impl Terminator {
    /// Every kind, in declaration order.
    pub const ALL: [Self; 3] = [Self::FullStop, Self::Question, Self::Exclamation];

    /// The scalars this kind recognises, in code-point order.
    ///
    /// Together these three slices are the complete recognised set; they are
    /// pairwise disjoint and every scalar in them is a single `char`.
    #[must_use]
    pub const fn scalars(self) -> &'static [char] {
        match self {
            Self::FullStop => &FULL_STOPS,
            Self::Question => &QUESTIONS,
            Self::Exclamation => &EXCLAMATIONS,
        }
    }

    /// Classifies a single Unicode scalar value.
    ///
    /// ```
    /// use verbora_analyzers::Terminator;
    ///
    /// assert_eq!(Terminator::from_scalar('.'), Some(Terminator::FullStop));
    /// assert_eq!(Terminator::from_scalar('a'), None);
    /// ```
    #[must_use]
    pub fn from_scalar(scalar: char) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|kind| kind.scalars().contains(&scalar))
    }

    /// Classifies a whole token.
    ///
    /// Returns `Some` only when the token is exactly one scalar and that scalar
    /// is recognised. The token is not trimmed, folded or normalised first —
    /// `" ."` and `".\u{fe0f}"` are two scalars and therefore not terminators.
    #[must_use]
    pub fn from_token(token: &str) -> Option<Self> {
        let mut scalars = token.chars();
        let first = scalars.next()?;
        if scalars.next().is_some() {
            return None;
        }
        Self::from_scalar(first)
    }

    /// A short, stable name for the kind. Not one of the scalars.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::FullStop => "full stop",
            Self::Question => "question mark",
            Self::Exclamation => "exclamation mark",
        }
    }
}

impl fmt::Display for Terminator {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Walks **every** scalar of the specified table, in both directions, and
    /// checks the totals. The tables are the whole contract, so a sampled test
    /// would prove nothing about the entries it skipped.
    #[test]
    fn every_specified_scalar_round_trips() {
        let mut total = 0;
        for kind in Terminator::ALL {
            let scalars = kind.scalars();
            assert!(!scalars.is_empty(), "{kind} has no scalars");
            for &scalar in scalars {
                assert_eq!(Terminator::from_scalar(scalar), Some(kind), "{scalar:?}");

                // A token is a terminator only via the same table.
                let token = scalar.to_string();
                assert_eq!(Terminator::from_token(&token), Some(kind), "{scalar:?}");
                assert_eq!(token.chars().count(), 1, "{scalar:?} is not one scalar");
            }
            total += scalars.len();
        }
        // 9 full stops + 9 question marks + 6 exclamation marks.
        assert_eq!(total, 24);
        assert_eq!(Terminator::FullStop.scalars().len(), 9);
        assert_eq!(Terminator::Question.scalars().len(), 9);
        assert_eq!(Terminator::Exclamation.scalars().len(), 6);
    }

    #[test]
    fn the_three_tables_are_disjoint_and_sorted() {
        let mut seen: Vec<char> = Vec::new();
        for kind in Terminator::ALL {
            let scalars = kind.scalars();
            let mut sorted = scalars.to_vec();
            sorted.sort_unstable();
            assert_eq!(sorted, scalars, "{kind} table is not in code-point order");
            for &scalar in scalars {
                assert!(!seen.contains(&scalar), "{scalar:?} appears in two tables");
                seen.push(scalar);
            }
        }
        assert_eq!(seen.len(), 24);
    }

    /// Each entry is pinned to the code point its Unicode name denotes, so a
    /// mis-transcribed table entry fails here rather than silently changing
    /// which tokens end a sentence.
    #[test]
    fn table_entries_are_the_named_code_points() {
        assert_eq!(
            Terminator::FullStop.scalars(),
            [
                '.',        // U+002E FULL STOP
                '\u{0589}', // ARMENIAN FULL STOP
                '\u{06D4}', // ARABIC FULL STOP
                '\u{0964}', // DEVANAGARI DANDA
                '\u{2024}', // ONE DOT LEADER
                '\u{3002}', // IDEOGRAPHIC FULL STOP
                '\u{FE52}', // SMALL FULL STOP
                '\u{FF0E}', // FULLWIDTH FULL STOP
                '\u{FF61}', // HALFWIDTH IDEOGRAPHIC FULL STOP
            ]
        );
        assert_eq!(
            Terminator::Question.scalars(),
            [
                '?',        // U+003F QUESTION MARK
                '\u{061F}', // ARABIC QUESTION MARK
                '\u{203D}', // INTERROBANG
                '\u{2047}', // DOUBLE QUESTION MARK
                '\u{2048}', // QUESTION EXCLAMATION MARK
                '\u{2049}', // EXCLAMATION QUESTION MARK
                '\u{2E2E}', // REVERSED QUESTION MARK
                '\u{FE56}', // SMALL QUESTION MARK
                '\u{FF1F}', // FULLWIDTH QUESTION MARK
            ]
        );
        assert_eq!(
            Terminator::Exclamation.scalars(),
            [
                '!',        // U+0021 EXCLAMATION MARK
                '\u{055C}', // ARMENIAN EXCLAMATION MARK
                '\u{07F9}', // NKO EXCLAMATION MARK
                '\u{203C}', // DOUBLE EXCLAMATION MARK
                '\u{FE57}', // SMALL EXCLAMATION MARK
                '\u{FF01}', // FULLWIDTH EXCLAMATION MARK
            ]
        );
    }

    /// The exclusions are part of the contract, so they are pinned too.
    #[test]
    fn documented_exclusions_are_not_terminators() {
        for token in [
            "",         // zero scalars
            " ",        // space
            " .",       // not trimmed
            ". ",       // not trimmed
            "..",       // two scalars
            "...",      // three scalars
            "?!",       // two scalars
            "\u{2026}", // HORIZONTAL ELLIPSIS
            "\u{00BF}", // INVERTED QUESTION MARK
            "\u{00A1}", // INVERTED EXCLAMATION MARK
            "\u{01C3}", // LATIN LETTER RETROFLEX CLICK
            "\u{003B}", // SEMICOLON
            "\u{003A}", // COLON
            ",",
            "a",
            "😀",
            ".\u{fe0f}", // FULL STOP + VARIATION SELECTOR-16
        ] {
            assert_eq!(Terminator::from_token(token), None, "{token:?}");
        }
    }

    /// Every scalar of the table also fails when it is only *part* of a token —
    /// the analyzer never looks inside a token for a mark.
    #[test]
    fn a_terminator_inside_a_longer_token_is_not_recognised() {
        for kind in Terminator::ALL {
            for &scalar in kind.scalars() {
                assert_eq!(Terminator::from_token(&format!("a{scalar}")), None);
                assert_eq!(Terminator::from_token(&format!("{scalar}a")), None);
                assert_eq!(Terminator::from_token(&format!("{scalar}{scalar}")), None);
            }
        }
    }

    #[test]
    fn names_are_unique() {
        let mut names: Vec<&str> = Terminator::ALL.iter().map(|k| k.name()).collect();
        names.sort_unstable();
        let before = names.len();
        names.dedup();
        assert_eq!(names.len(), before);
        assert_eq!(Terminator::Question.to_string(), "question mark");
    }
}
