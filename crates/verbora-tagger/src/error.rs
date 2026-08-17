//! The errors the reference throws out of the Brill tagger.
//!
//! Throwing is part of the observable contract: `fixtures/tagger.json` records
//! 348 cases whose recorded outcome is an exception, and several of them are
//! reachable from ordinary input (an empty token under `CURRENT-WORD-IS-CAP`, a
//! `PREV-WORD-IS` rule written without its parameter, a lexicon text with no
//! non-newline characters). The [`Display`](std::fmt::Display) text of every variant is the exact
//! `Error.message` the reference engine produces, so a parity suite can compare messages rather
//! than merely asserting "something failed".

use std::fmt;

/// An error thrown by a Brill tagger operation.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum TaggerError {
    /// A property was read from `undefined` — the shape of nearly every
    /// TypeError in this module. `object` is the reference expression the reference engine names
    /// (`undefined` or `null`) and `property` the key being read.
    ReadOfUndefined {
        /// `"undefined"` or `"null"`, as the reference engine reports it.
        object: &'static str,
        /// The property name that was read.
        property: &'static str,
    },
    /// `this.meta.function` was not callable.
    ///
    /// Reached when a predicate is named after an `Object.prototype` member:
    /// `ruleTemplates['toString']` finds the inherited function, so the
    /// `if (!this.meta)` fallback to `DEFAULT` never fires and `meta.function`
    /// is `undefined`.
    PredicateNotAFunction,
    /// `word.toLowerCase is not a function` — `Lexicon.tagWord` was handed a
    /// non-string whose key missed the dictionary.
    WordNotAString,
    /// `sentence.forEach is not a function` — `tag()` was handed a non-array.
    SentenceNotAnArray,
    /// `sentence.generateFeatures is not a function` — `Corpus.generateFeatures`
    /// calls a method `Sentence` does not define, so it always throws.
    GenerateFeaturesMissing,
    /// `Cannot convert undefined or null to object` — `Corpus.getTags()` before
    /// `analyse()` has created `posTags`.
    ObjectKeysOfUndefined,
    /// A rule string did not parse. See [`crate::parser::SyntaxError`].
    Syntax(crate::parser::SyntaxError),
}

impl fmt::Display for TaggerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ReadOfUndefined { object, property } => {
                write!(
                    f,
                    "Cannot read properties of {object} (reading '{property}')"
                )
            }
            Self::PredicateNotAFunction => f.write_str("predicate is not a function"),
            Self::WordNotAString => f.write_str("word.toLowerCase is not a function"),
            Self::SentenceNotAnArray => f.write_str("sentence.forEach is not a function"),
            Self::GenerateFeaturesMissing => {
                f.write_str("sentence.generateFeatures is not a function")
            }
            Self::ObjectKeysOfUndefined => {
                f.write_str("Cannot convert undefined or null to object")
            }
            Self::Syntax(e) => fmt::Display::fmt(e, f),
        }
    }
}

impl std::error::Error for TaggerError {}

impl From<crate::parser::SyntaxError> for TaggerError {
    fn from(e: crate::parser::SyntaxError) -> Self {
        Self::Syntax(e)
    }
}

impl TaggerError {
    /// `Cannot read properties of undefined (reading '<property>')`.
    pub(crate) const fn undefined(property: &'static str) -> Self {
        Self::ReadOfUndefined {
            object: "undefined",
            property,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn messages_match_the_specification() {
        assert_eq!(
            TaggerError::undefined("toUpperCase").to_string(),
            "Cannot read properties of undefined (reading 'toUpperCase')"
        );
        assert_eq!(
            TaggerError::PredicateNotAFunction.to_string(),
            "predicate is not a function"
        );
        assert_eq!(
            TaggerError::ObjectKeysOfUndefined.to_string(),
            "Cannot convert undefined or null to object"
        );
    }
}
