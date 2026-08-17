//! Annotated corpora: parsing, analysis, and lexicon induction.

use std::borrow::Cow;

use crate::error::TaggerError;
use crate::lexicon::Lexicon;
use crate::ordered_object::OrderedObject;
use crate::sentence::{Prop, Sentence, Tag, TaggedWord};

/// The string key an `undefined` tag becomes when used as an object key.
///
/// `posTags[token.tag] = true` coerces the key, so a corpus with untagged tokens
/// really does grow a POS tag literally spelled `"undefined"`, and
/// `buildLexicon` can emit it as a category.
const UNDEFINED_KEY: &str = "undefined";

/// A corpus of tagged sentences.
#[derive(Debug, Clone, Default)]
pub struct Corpus<'a> {
    /// The sentences.
    pub sentences: Vec<Sentence<'a>>,
    /// Running word count. Set by the constructors and recomputed by
    /// [`Corpus::analyse`] — but **not** by
    /// [`Corpus::split_in_train_and_test`], which is why both halves of a split
    /// report zero.
    pub word_count: usize,
    /// Set by [`Corpus::analyse`]: token → tag → frequency.
    tag_frequencies: Option<OrderedObject<OrderedObject<u32>>>,
    /// Set by [`Corpus::analyse`]: the set of tags used.
    pos_tags: Option<OrderedObject<()>>,
}

impl<'a> Corpus<'a> {
    /// An empty corpus — `new Corpus()`.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Builds a corpus from already-parsed sentences, the referenceON path.
    ///
    /// The reference stores the caller's `taggedWords` arrays **by reference**, so
    /// training mutates the parsed JSON in place. Rust's ownership rules make
    /// that impossible; the values are moved here instead. Nothing observable
    /// through this crate's API depends on the aliasing, but a caller that
    /// inspects its own input after training will see it unchanged where
    /// the reference would show it rewritten.
    #[must_use]
    pub fn from_sentences(sentences: Vec<Sentence<'a>>) -> Self {
        let word_count = sentences.iter().map(Sentence::len).sum();
        Self {
            sentences,
            word_count,
            ..Self::default()
        }
    }

    /// `parseBrownCorpus(data)`: one sentence per non-blank line, tokens of the
    /// form `word_TAG`.
    ///
    /// Splitting is on `\n` only, so a CRLF file leaves `\r` attached to the last
    /// token of each line — except that `line.trim()` removes it before the split.
    /// A token with no underscore gets **no tag**; `a_b_c_d` keeps only `a` and
    /// `b`; a token starting with `_` yields an empty token string. Blank and
    /// whitespace-only lines are skipped and do not count towards the word count.
    #[must_use]
    pub fn parse_brown(data: &'a str) -> Self {
        let mut corpus = Self::new();
        for line in data.split('\n') {
            let trimmed = line.trim_matches(verbora_core::whitespace::is_whitespace);
            if trimmed.is_empty() {
                continue;
            }
            let mut sentence = Sentence::new();
            for token in trimmed.split(verbora_core::whitespace::is_whitespace) {
                if token.is_empty() {
                    // `split(/\s+/)` on a trimmed, non-empty string never yields
                    // an empty field, so this cannot happen; the guard keeps the
                    // char-wise split above equivalent to the regex one.
                    continue;
                }
                corpus.word_count += 1;
                let mut parts = token.split('_');
                let word = parts.next().unwrap_or("");
                // The token borrows from `data`, but a `Tag` is `Cow<'static>` —
                // tags are almost always `&'static str` from the bundled tables,
                // and widening the alias would cost every hot path a lifetime
                // parameter to spare this one parser an allocation per token.
                let tag = parts.next().map(|t| Cow::Owned(t.to_string()));
                sentence.add_tagged_word(word, tag);
            }
            corpus.sentences.push(sentence);
        }
        corpus
    }

    /// `nrSentences()`.
    #[must_use]
    pub fn len(&self) -> usize {
        self.sentences.len()
    }

    /// Whether the corpus has no sentences.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.sentences.is_empty()
    }

    /// `nrWords()`.
    #[must_use]
    pub const fn word_count(&self) -> usize {
        self.word_count
    }

    /// `analyse()`: recomputes the word count, the tag set and the per-token tag
    /// frequencies.
    ///
    /// All three are reset first, so calling it twice is idempotent.
    pub fn analyse(&mut self) {
        let mut tag_frequencies: OrderedObject<OrderedObject<u32>> = OrderedObject::new();
        let mut pos_tags: OrderedObject<()> = OrderedObject::new();
        let mut word_count = 0;
        for sentence in &self.sentences {
            for token in &sentence.tagged_words {
                word_count += 1;
                let tag_key = token.tag().unwrap_or(UNDEFINED_KEY);
                pos_tags.insert(tag_key, ());
                let per_token = tag_frequencies.entry_or_insert(&token.token, OrderedObject::new());
                *per_token.entry_or_insert(tag_key, 0) += 1;
            }
        }
        self.word_count = word_count;
        self.tag_frequencies = Some(tag_frequencies);
        self.pos_tags = Some(pos_tags);
    }

    /// `getTags()`: the tags seen, in `Object.keys` order.
    ///
    /// # Errors
    ///
    /// Before [`Corpus::analyse`] has run, `posTags` is `undefined` and
    /// `Object.keys` throws.
    pub fn get_tags(&self) -> Result<Vec<&str>, TaggerError> {
        self.pos_tags
            .as_ref()
            .map(OrderedObject::keys)
            .ok_or(TaggerError::ObjectKeysOfUndefined)
    }

    /// The per-token tag frequencies, once [`Corpus::analyse`] has run.
    #[must_use]
    pub fn tag_frequencies(&self) -> Option<&OrderedObject<OrderedObject<u32>>> {
        self.tag_frequencies.as_ref()
    }

    /// `tag(lexicon)`: writes each token's `testTag` from the lexicon.
    ///
    /// The gold `tag` is untouched. `testTag` becomes `undefined` where the
    /// lexicon returns an empty category list.
    pub fn tag(&mut self, lexicon: &Lexicon) {
        for sentence in &mut self.sentences {
            for token in &mut sentence.tagged_words {
                token.test_tag = Prop::assigned(lexicon.first_category(&token.token));
            }
        }
    }

    /// `buildLexicon()`: the most frequent tag of each token, most frequent first.
    ///
    /// # This does not return an empty lexicon
    ///
    /// The reference calls `new Lexicon()` **with no arguments**, and `Lexicon`'s
    /// language switch defaults to **Dutch** — so the returned "corpus lexicon"
    /// carries all 11,699 Dutch words, has no default category, and *is* the
    /// process-global Dutch dictionary, which this call permanently pollutes with
    /// the corpus vocabulary. Tagging an English corpus with it therefore falls
    /// back on Dutch entries for words the corpus never contained.
    ///
    /// That is almost certainly an upstream bug — every plausible reading of the
    /// method says it should build a lexicon *from the corpus* — but it changes
    /// `nrEntries()`, the tags of out-of-vocabulary words, and every downstream
    /// training and testing score, so it is reproduced rather than corrected. Use
    /// [`Corpus::build_lexicon_detached`] for the behaviour the name implies.
    pub fn build_lexicon(&mut self) -> Lexicon {
        self.build_lexicon_into(Lexicon::new(None, None, None))
    }

    /// [`Corpus::build_lexicon`] without the Dutch dictionary or the global
    /// mutation: a lexicon containing exactly the corpus vocabulary.
    ///
    /// No the reference equivalent. Provided because the reference's behaviour is a
    /// bug that callers should be able to opt out of without giving up parity
    /// elsewhere.
    pub fn build_lexicon_detached(&mut self) -> Lexicon {
        self.build_lexicon_into(Lexicon::empty())
    }

    fn build_lexicon_into(&mut self, mut lexicon: Lexicon) -> Lexicon {
        self.analyse();
        let frequencies = self
            .tag_frequencies
            .as_ref()
            .expect("analyse just ran")
            .in_key_order();
        // Collected first so the borrow of `self` ends before `add_word`.
        let mut updates: Vec<(String, Vec<Tag>)> = Vec::with_capacity(frequencies.len());
        for (token, per_tag) in frequencies {
            let mut cats: Vec<(&str, u32)> = per_tag
                .in_key_order()
                .into_iter()
                .map(|(t, n)| (t, *n))
                .collect();
            // The reference engine's sort is stable and the comparator returns 0 for ties, so
            // equal-frequency tags keep their first-seen order. Stable, never
            // `sort_unstable_by_key`.
            cats.sort_by_key(|c| std::cmp::Reverse(c.1));
            updates.push((
                token.to_string(),
                cats.into_iter()
                    .map(|(t, _)| Cow::Owned(t.to_string()))
                    .collect(),
            ));
        }
        for (token, cats) in updates {
            lexicon.add_word(&token, cats);
        }
        lexicon
    }

    /// `splitInTrainAndTest(percentageTrain)`.
    ///
    /// `next_random` stands in for `Math.random()`; the reference uses the
    /// unseeded global generator, which makes the split non-reproducible. Two
    /// things about the result are deterministic and are preserved: the
    /// sentences are conserved, and **`wordCount` is never updated**, so both
    /// halves report `word_count() == 0`.
    pub fn split_in_train_and_test(
        &self,
        percentage_train: f64,
        mut next_random: impl FnMut() -> f64,
    ) -> (Corpus<'a>, Corpus<'a>) {
        let p = percentage_train / 100.0;
        let mut train = Corpus::new();
        let mut test = Corpus::new();
        for sentence in &self.sentences {
            if next_random() < p {
                train.sentences.push(sentence.clone());
            } else {
                test.sentences.push(sentence.clone());
            }
        }
        (train, test)
    }

    /// `generateFeatures()`.
    ///
    /// # Errors
    ///
    /// Always. The reference calls `sentence.generateFeatures(features)`, and
    /// `Sentence` has no such method, so this method cannot succeed for any
    /// non-empty corpus. It is reproduced because a caller reaching for it needs
    /// to learn that, not to receive an empty list.
    pub fn generate_features(&self) -> Result<Vec<String>, TaggerError> {
        if self.sentences.is_empty() {
            return Ok(Vec::new());
        }
        Err(TaggerError::GenerateFeaturesMissing)
    }

    /// `prettyPrint()`: The reference's body is entirely commented out, so this
    /// produces nothing. Kept so the surface matches.
    pub const fn pretty_print(&self) {}

    /// Detaches every token from the text it borrows.
    #[must_use]
    pub fn into_owned(self) -> Corpus<'static> {
        Corpus {
            sentences: self
                .sentences
                .into_iter()
                .map(Sentence::into_owned)
                .collect(),
            word_count: self.word_count,
            tag_frequencies: None,
            pos_tags: None,
        }
    }
}

impl<'a> FromIterator<Sentence<'a>> for Corpus<'a> {
    fn from_iter<T: IntoIterator<Item = Sentence<'a>>>(iter: T) -> Self {
        Self::from_sentences(iter.into_iter().collect())
    }
}

/// Convenience for building a sentence from `(token, tag)` pairs.
#[must_use]
pub fn sentence_from_pairs<'a>(pairs: &[(&'a str, Option<&'a str>)]) -> Sentence<'a> {
    Sentence::from_words(
        pairs
            .iter()
            .map(|(t, g)| TaggedWord::new(*t, g.map(|s| Cow::Owned(s.to_string()))))
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn brown_parsing_shapes() {
        let c = Corpus::parse_brown(
            "The_AT dog_NN runs_VBZ\n\n  \n Bad_JJ token_NN\nnoTag oneMore_XX\na_b_c_d\n",
        );
        assert_eq!(c.len(), 4);
        assert_eq!(c.word_count(), 8);
        // No underscore: no tag at all.
        assert_eq!(c.sentences[2].tagged_words[0].tag(), None);
        // Extra segments are discarded.
        assert_eq!(c.sentences[3].tagged_words[0].token, "a");
        assert_eq!(c.sentences[3].tagged_words[0].tag(), Some("b"));
    }

    #[test]
    fn brown_parsing_edge_inputs() {
        for input in ["", "\n", "   \n\t\n"] {
            let c = Corpus::parse_brown(input);
            assert!(c.is_empty(), "{input:?}");
            assert_eq!(c.word_count(), 0);
        }
        let c = Corpus::parse_brown("_leading NN_\n");
        assert_eq!(c.sentences[0].tagged_words[0].token, "");
        assert_eq!(c.sentences[0].tagged_words[1].tag(), Some(""));
    }

    #[test]
    fn analyse_hoists_numeric_token_keys() {
        let mut c = Corpus::from_sentences(vec![sentence_from_pairs(&[
            ("2", Some("CD")),
            ("a", Some("NN")),
            ("a", Some("JJ")),
            ("a", Some("NN")),
            ("1", Some("CD")),
        ])]);
        c.analyse();
        assert_eq!(c.tag_frequencies().unwrap().keys(), ["1", "2", "a"]);
        assert_eq!(c.get_tags().unwrap(), ["CD", "NN", "JJ"]);
        assert_eq!(c.word_count(), 5);
    }

    #[test]
    fn undefined_tags_become_a_literal_key() {
        let mut c =
            Corpus::from_sentences(vec![sentence_from_pairs(&[("x", None), ("y", Some("NN"))])]);
        c.analyse();
        assert_eq!(c.get_tags().unwrap(), ["undefined", "NN"]);
    }

    #[test]
    fn get_tags_requires_analyse() {
        let c = Corpus::from_sentences(vec![sentence_from_pairs(&[("x", Some("NN"))])]);
        assert_eq!(c.get_tags(), Err(TaggerError::ObjectKeysOfUndefined));
    }

    #[test]
    fn generate_features_always_fails() {
        let c = Corpus::from_sentences(vec![sentence_from_pairs(&[("x", Some("NN"))])]);
        assert_eq!(
            c.generate_features(),
            Err(TaggerError::GenerateFeaturesMissing)
        );
    }

    #[test]
    fn split_conserves_sentences_but_not_the_word_count() {
        let c = Corpus::from_sentences(vec![
            sentence_from_pairs(&[("a", Some("NN"))]),
            sentence_from_pairs(&[("b", Some("NN"))]),
            sentence_from_pairs(&[("c", Some("NN"))]),
        ]);
        let mut seq = [0.1_f64, 0.9, 0.2].into_iter().cycle();
        let (train, test) = c.split_in_train_and_test(60.0, || seq.next().expect("cycles"));
        assert_eq!(train.len() + test.len(), c.len());
        assert_eq!(train.word_count(), 0);
        assert_eq!(test.word_count(), 0);
        assert_eq!(c.word_count(), 3);
    }

    #[test]
    fn build_lexicon_orders_categories_by_frequency() {
        let mut c = Corpus::from_sentences(vec![sentence_from_pairs(&[
            ("run", Some("VB")),
            ("run", Some("NN")),
            ("run", Some("VB")),
        ])]);
        let lex = c.build_lexicon_detached();
        assert_eq!(lex.tag_word("run").to_vec(), [Some("VB"), Some("NN")]);
    }

    #[test]
    fn build_lexicon_keeps_ties_in_first_seen_order() {
        let mut c = Corpus::from_sentences(vec![sentence_from_pairs(&[
            ("x", Some("BB")),
            ("x", Some("AA")),
        ])]);
        let lex = c.build_lexicon_detached();
        assert_eq!(lex.tag_word("x").to_vec(), [Some("BB"), Some("AA")]);
    }
}
