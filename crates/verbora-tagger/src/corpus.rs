//! An annotated corpus: sentences of tokens with their gold tags.

use std::fmt;

use rustc_hash::FxHashMap;

use crate::lexicon::{Lexicon, LexiconError};
use crate::tag::{LiteralError, Tag, TaggedToken};

/// Why a Brown-format corpus did not parse.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum CorpusParseError {
    /// A token carried no `_` and therefore no tag.
    ///
    /// A corpus is *annotated* by definition; an untagged token would have to be
    /// given an invented tag or a missing one, and both are worse than saying so.
    MissingTag {
        /// One-based line number.
        line: usize,
        /// The offending token.
        token: String,
    },
    /// The text before the final `_` was empty.
    EmptyToken {
        /// One-based line number.
        line: usize,
        /// The offending token.
        token: String,
    },
    /// The text after the final `_` was empty.
    EmptyTag {
        /// One-based line number.
        line: usize,
        /// The offending token.
        token: String,
    },
    /// The text after the final `_` was `*`, which is the wildcard pattern and
    /// so not a tag. See [`LiteralError::Wildcard`](crate::LiteralError::Wildcard).
    WildcardTag {
        /// One-based line number.
        line: usize,
        /// The offending token.
        token: String,
    },
}

impl fmt::Display for CorpusParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingTag { line, token } => {
                write!(f, "line {line}: token {token:?} has no '_' and so no tag")
            }
            Self::EmptyToken { line, token } => {
                write!(f, "line {line}: token {token:?} has an empty word")
            }
            Self::EmptyTag { line, token } => {
                write!(f, "line {line}: token {token:?} has an empty tag")
            }
            Self::WildcardTag { line, token } => write!(
                f,
                "line {line}: token {token:?} is tagged \"*\", which is the wildcard pattern"
            ),
        }
    }
}

impl std::error::Error for CorpusParseError {}

/// Sentences of gold-tagged tokens.
///
/// Tokens borrow from the text the corpus was parsed from, so parsing copies
/// only the tag strings.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Corpus<'a> {
    sentences: Vec<Vec<TaggedToken<'a>>>,
}

impl<'a> Corpus<'a> {
    /// An empty corpus.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Wraps already-built sentences.
    #[must_use]
    pub const fn from_sentences(sentences: Vec<Vec<TaggedToken<'a>>>) -> Self {
        Self { sentences }
    }

    /// Parses the Brown-corpus text format: one sentence per line, tokens of the
    /// form `word_TAG` separated by whitespace.
    ///
    /// The split is at the **last** `_` in the token, so `node_js_NN` is the
    /// token `node_js` tagged `NN` — nothing is discarded. Lines are split on
    /// `\n`, and each line is trimmed, so a CRLF file parses identically to an
    /// LF one. Blank and whitespace-only lines are skipped.
    ///
    /// Tokens are whitespace-delimited, which is exactly the [`Lexicon`] key
    /// contract, so a lexicon built from a corpus is always reachable from the
    /// same tokenization the corpus used.
    ///
    /// # Errors
    ///
    /// [`CorpusParseError`] for a token with no `_`, an empty word, an empty tag
    /// or the tag `*`. Nothing is silently dropped or silently retagged.
    pub fn parse_brown(text: &'a str) -> Result<Self, CorpusParseError> {
        let mut sentences = Vec::new();
        for (n, line) in text.split('\n').enumerate() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            let mut sentence = Vec::new();
            for token in line.split_whitespace() {
                let Some((word, tag)) = token.rsplit_once('_') else {
                    return Err(CorpusParseError::MissingTag {
                        line: n + 1,
                        token: token.to_owned(),
                    });
                };
                if word.is_empty() {
                    return Err(CorpusParseError::EmptyToken {
                        line: n + 1,
                        token: token.to_owned(),
                    });
                }
                let tag = Tag::new(tag.to_owned()).map_err(|cause| match cause {
                    LiteralError::Wildcard => CorpusParseError::WildcardTag {
                        line: n + 1,
                        token: token.to_owned(),
                    },
                    // Whitespace cannot occur: the token came from
                    // `split_whitespace`. What is left is the empty tag.
                    _ => CorpusParseError::EmptyTag {
                        line: n + 1,
                        token: token.to_owned(),
                    },
                })?;
                sentence.push(TaggedToken::new(word, tag));
            }
            sentences.push(sentence);
        }
        Ok(Self { sentences })
    }

    /// The sentences, in order.
    #[inline]
    #[must_use]
    pub fn sentences(&self) -> &[Vec<TaggedToken<'a>>] {
        &self.sentences
    }

    /// Mutable access to the sentences.
    #[inline]
    pub fn sentences_mut(&mut self) -> &mut Vec<Vec<TaggedToken<'a>>> {
        &mut self.sentences
    }

    /// Number of sentences.
    #[must_use]
    pub fn len(&self) -> usize {
        self.sentences.len()
    }

    /// Whether the corpus has no sentences.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.sentences.is_empty()
    }

    /// Number of tokens across all sentences.
    ///
    /// Computed from the sentences every time it is asked for, so it can never
    /// disagree with them.
    #[must_use]
    pub fn token_count(&self) -> usize {
        self.sentences.iter().map(Vec::len).sum()
    }

    /// The distinct tags used, in first-appearance order.
    #[must_use]
    pub fn tags(&self) -> Vec<&Tag> {
        let mut seen: Vec<&Tag> = Vec::new();
        for sentence in &self.sentences {
            for w in sentence {
                if !seen.contains(&&w.tag) {
                    seen.push(&w.tag);
                }
            }
        }
        seen
    }

    /// Builds a [`Lexicon`] from the corpus vocabulary alone.
    ///
    /// Each token maps to the tags it was annotated with, **most frequent
    /// first**, which is what makes [`Lexicon::primary_tag`] the most-likely-tag
    /// annotator. Ties keep first-appearance order, so the result is
    /// deterministic for a given corpus.
    ///
    /// The returned lexicon contains nothing but this corpus: no shared state,
    /// and no tokens the corpus did not contain.
    ///
    /// # Errors
    ///
    /// [`LexiconError`] when a corpus token is not a conforming lexicon key —
    /// empty, or containing whitespace. [`Corpus::parse_brown`] cannot produce
    /// one; [`Corpus::from_sentences`] can.
    pub fn build_lexicon(&self, default_tag: Tag) -> Result<Lexicon, LexiconError> {
        let mut counts: FxHashMap<&str, Vec<(Tag, usize)>> = FxHashMap::default();
        for sentence in &self.sentences {
            for w in sentence {
                let entry = counts.entry(w.token()).or_default();
                match entry.iter_mut().find(|(t, _)| *t == w.tag) {
                    Some((_, n)) => *n += 1,
                    None => entry.push((w.tag.clone(), 1)),
                }
            }
        }
        let mut lexicon = Lexicon::new(default_tag);
        for (token, mut tags) in counts {
            // Stable, so equal counts keep first-appearance order.
            tags.sort_by_key(|(_, n)| std::cmp::Reverse(*n));
            lexicon.insert(token, tags.into_iter().map(|(t, _)| t).collect())?;
        }
        Ok(lexicon)
    }

    /// Splits the corpus in two by a caller-supplied predicate.
    ///
    /// The predicate receives each sentence's index and contents and returns
    /// `true` to put it in the first half. Verbora supplies no randomness: a
    /// library that reached for a global generator would make the split
    /// irreproducible, so the choice — round-robin, a seeded generator, a
    /// document boundary — stays with the caller.
    ///
    /// ```
    /// use verbora_tagger::Corpus;
    ///
    /// let corpus = Corpus::parse_brown("a_A\nb_B\nc_C\nd_D")?;
    /// let (train, test) = corpus.partition(|i, _| i % 4 != 3);
    /// assert_eq!(train.len(), 3);
    /// assert_eq!(test.len(), 1);
    /// assert_eq!(train.token_count() + test.token_count(), corpus.token_count());
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    #[must_use]
    pub fn partition(
        &self,
        mut first: impl FnMut(usize, &[TaggedToken<'a>]) -> bool,
    ) -> (Self, Self) {
        let mut a = Vec::new();
        let mut b = Vec::new();
        for (i, sentence) in self.sentences.iter().enumerate() {
            if first(i, sentence) {
                a.push(sentence.clone());
            } else {
                b.push(sentence.clone());
            }
        }
        (Self { sentences: a }, Self { sentences: b })
    }

    /// Detaches every token from the text it borrows.
    #[must_use]
    pub fn into_owned(self) -> Corpus<'static> {
        Corpus {
            sentences: self
                .sentences
                .into_iter()
                .map(|s| s.into_iter().map(TaggedToken::into_owned).collect())
                .collect(),
        }
    }
}

impl<'a> FromIterator<Vec<TaggedToken<'a>>> for Corpus<'a> {
    fn from_iter<T: IntoIterator<Item = Vec<TaggedToken<'a>>>>(iter: T) -> Self {
        Self::from_sentences(iter.into_iter().collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tag(s: &'static str) -> Tag {
        Tag::new(s).unwrap()
    }

    #[test]
    fn brown_parsing() {
        let c = Corpus::parse_brown("The_AT dog_NN runs_VBZ\n\n  \nBad_JJ token_NN\n").unwrap();
        assert_eq!(c.len(), 2);
        assert_eq!(c.token_count(), 5);
        assert_eq!(c.sentences()[0][0].token(), "The");
        assert_eq!(c.sentences()[0][0].tag(), &tag("AT"));
    }

    /// The split is at the last `_`, so nothing is discarded.
    #[test]
    fn multi_underscore_tokens_keep_their_word() {
        let c = Corpus::parse_brown("node_js_NN a_b_c_d").unwrap();
        assert_eq!(c.sentences()[0][0].token(), "node_js");
        assert_eq!(c.sentences()[0][0].tag(), &tag("NN"));
        assert_eq!(c.sentences()[0][1].token(), "a_b_c");
        assert_eq!(c.sentences()[0][1].tag(), &tag("d"));
    }

    #[test]
    fn malformed_tokens_are_reported_not_dropped() {
        assert_eq!(
            Corpus::parse_brown("The_AT noTag"),
            Err(CorpusParseError::MissingTag {
                line: 1,
                token: "noTag".to_owned()
            })
        );
        assert_eq!(
            Corpus::parse_brown("a_A\n_leading"),
            Err(CorpusParseError::EmptyToken {
                line: 2,
                token: "_leading".to_owned()
            })
        );
        assert_eq!(
            Corpus::parse_brown("NN_"),
            Err(CorpusParseError::EmptyTag {
                line: 1,
                token: "NN_".to_owned()
            })
        );
        // `*` is the wildcard pattern, not a tag, so a corpus that annotates a
        // token with it is reported rather than turned into a rule that would
        // rewrite every tag once trained.
        assert_eq!(
            Corpus::parse_brown("a_A\nx_*"),
            Err(CorpusParseError::WildcardTag {
                line: 2,
                token: "x_*".to_owned()
            })
        );
        assert_eq!(
            Corpus::parse_brown("x_*").unwrap_err().to_string(),
            "line 1: token \"x_*\" is tagged \"*\", which is the wildcard pattern"
        );
    }

    #[test]
    fn empty_inputs() {
        for input in ["", "\n", "   \n\t\n", "\r\n"] {
            let c = Corpus::parse_brown(input).unwrap();
            assert!(c.is_empty(), "{input:?}");
            assert_eq!(c.token_count(), 0);
        }
    }

    #[test]
    fn crlf_parses_like_lf() {
        let a = Corpus::parse_brown("a_A b_B\nc_C d_D\n").unwrap();
        let b = Corpus::parse_brown("a_A b_B\r\nc_C d_D\r\n").unwrap();
        assert_eq!(a, b);
    }

    #[test]
    fn build_lexicon_orders_by_frequency_then_first_appearance() {
        let c = Corpus::parse_brown("run_VB run_NN run_VB\nx_BB x_AA").unwrap();
        let lex = c.build_lexicon(tag("NN")).unwrap();
        assert_eq!(
            lex.tags("run").unwrap().collect::<Vec<_>>(),
            [tag("VB"), tag("NN")]
        );
        assert_eq!(
            lex.tags("x").unwrap().collect::<Vec<_>>(),
            [tag("BB"), tag("AA")],
            "equal counts keep first-appearance order"
        );
        assert_eq!(lex.len(), 2, "only the corpus vocabulary");
        assert!(!lex.contains("dog"), "nothing outside the corpus leaked in");
    }

    #[test]
    fn build_lexicon_rejects_non_conforming_tokens() {
        let c = Corpus::from_sentences(vec![vec![TaggedToken::new("a b", tag("NN"))]]);
        assert!(c.build_lexicon(tag("NN")).is_err());
    }

    #[test]
    fn partition_conserves_everything() {
        let c = Corpus::parse_brown("a_A\nb_B\nc_C\nd_D").unwrap();
        let (train, test) = c.partition(|i, _| i % 2 == 0);
        assert_eq!(train.len(), 2);
        assert_eq!(test.len(), 2);
        assert_eq!(train.token_count() + test.token_count(), c.token_count());
        assert_eq!(c.token_count(), 4, "the source is untouched");
    }

    #[test]
    fn tags_are_in_first_appearance_order() {
        let c = Corpus::parse_brown("x_CD a_NN a_JJ a_NN y_CD").unwrap();
        assert_eq!(
            c.tags().into_iter().cloned().collect::<Vec<_>>(),
            [tag("CD"), tag("NN"), tag("JJ")]
        );
    }
}
