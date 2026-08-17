//! Part-of-speech tagging: `POSElement`, `MESentence` and `MECorpus`.
//!
//! The context of a POS event is a window of words and a window of tags around
//! the token being tagged, and the class is its tag. Three feature families are
//! live; six more are present in the reference source but commented out, and
//! restoring them would change the feature count and the entire model.

use std::rc::Rc;

use verbora_core::whitespace::is_whitespace;

use crate::dynval::DynValue;
use crate::maxent::MaxEntError;
use crate::maxent::context::Context;
use crate::maxent::element::{Element, ElementKind};
use crate::maxent::feature::Feature;
use crate::maxent::feature_set::FeatureSet;
use crate::maxent::sample::Sample;

/// Constructs elements whose features are the POS-tagging family.
#[derive(Debug, Clone, Copy)]
pub struct POSElement;

impl POSElement {
    /// An element whose `generateFeatures` adds the POS features.
    ///
    /// `b`'s payload must be `{ wordWindow: {…}, tagWindow: {…} }`.
    ///
    /// Returns an [`Element`], not a `POSElement`: in the reference `POSElement`
    /// *is* an `Element` — a subclass adding one method — and modelling that as
    /// a separate Rust type would fork every API that takes an element.
    #[allow(clippy::new_ret_no_self)]
    pub fn new(a: impl Into<String>, b: Rc<Context>) -> Element {
        Element::with_kind(a, b, ElementKind::Pos)
    }
}

/// Reads `data.<window>['<offset>']`.
fn window_entry(data: &DynValue, window: &str, offset: &str) -> Option<DynValue> {
    data.get(window)?.get(offset).cloned()
}

/// `String(value)` for a captured window entry, with `undefined` for absent —
/// which is what the reference bakes into a feature's `parametersKey` when the
/// window has no such offset.
fn param(value: Option<&DynValue>) -> String {
    value.map_or_else(|| "undefined".to_owned(), DynValue::to_text)
}

/// The reference `===` for two optional window entries; absent is `undefined`,
/// and `undefined === undefined` is true.
fn strict_eq(a: Option<&DynValue>, b: Option<&DynValue>) -> bool {
    match (a, b) {
        (None, None) => true,
        (Some(x), Some(y)) => match (x, y) {
            // NaN is never equal to itself, in either language.
            (DynValue::Num(p), DynValue::Num(q)) => p == q,
            _ => x == y,
        },
        _ => false,
    }
}

/// `POSElement.prototype.generateFeatures`.
///
/// # Errors
///
/// [`MaxEntError::PosContextMissingWindows`] if the payload has no
/// `wordWindow` — the reference throws `TypeError` reading `['0']` of
/// `undefined`.
pub(crate) fn generate(element: &Element, feature_set: &mut FeatureSet) -> Result<(), MaxEntError> {
    let data = &element.b.data;
    if data.get("wordWindow").is_none() || data.get("tagWindow").is_none() {
        return Err(MaxEntError::PosContextMissingWindows);
    }
    let tag = element.a.clone();
    let token = window_entry(data, "wordWindow", "0");

    // 1. Always: the current word paired with the current tag.
    {
        let parameters = vec![
            "0".to_owned(),
            param(token.as_ref()),
            "0".to_owned(),
            tag.clone(),
        ];
        let captured_tag = tag.clone();
        let captured_token = token;
        let evaluate = Rc::new(move |x: &Element| {
            let seen = window_entry(&x.b.data, "wordWindow", "0");
            if strict_eq(seen.as_ref(), captured_token.as_ref()) && x.a == captured_tag {
                1.0
            } else {
                0.0
            }
        });
        feature_set.add_feature(Rc::new(Feature::new(evaluate, "wordFeature", parameters)));
    }

    // 2. The previous two tags — guarded on TRUTHINESS, so an empty-string tag
    //    at offset -2 skips the feature rather than adding it with an empty
    //    parameter.
    let prev_prev_tag = window_entry(data, "tagWindow", "-2");
    if prev_prev_tag.as_ref().is_some_and(DynValue::is_truthy) {
        let prev_tag = window_entry(data, "tagWindow", "-1");
        let parameters = vec![
            "0".to_owned(),
            tag.clone(),
            "-2".to_owned(),
            param(prev_prev_tag.as_ref()),
            "-1".to_owned(),
            param(prev_tag.as_ref()),
        ];
        let tag = tag.clone();
        let evaluate = Rc::new(move |x: &Element| {
            let seen_pp = window_entry(&x.b.data, "tagWindow", "-2");
            let seen_p = window_entry(&x.b.data, "tagWindow", "-1");
            if x.a == tag
                && strict_eq(seen_pp.as_ref(), prev_prev_tag.as_ref())
                && strict_eq(seen_p.as_ref(), prev_tag.as_ref())
            {
                1.0
            } else {
                0.0
            }
        });
        feature_set.add_feature(Rc::new(Feature::new(evaluate, "prevBigram", parameters)));
    }

    // 3. The previous word and its tag.
    let prev_word = window_entry(data, "wordWindow", "-1");
    if prev_word.as_ref().is_some_and(DynValue::is_truthy) {
        let prev_tag = window_entry(data, "tagWindow", "-1");
        let parameters = vec![
            "0".to_owned(),
            tag.clone(),
            "-1".to_owned(),
            param(prev_word.as_ref()),
            "-1".to_owned(),
            param(prev_tag.as_ref()),
        ];
        let evaluate = Rc::new(move |x: &Element| {
            let seen_w = window_entry(&x.b.data, "wordWindow", "-1");
            let seen_t = window_entry(&x.b.data, "tagWindow", "-1");
            if x.a == tag
                && strict_eq(seen_w.as_ref(), prev_word.as_ref())
                && strict_eq(seen_t.as_ref(), prev_tag.as_ref())
            {
                1.0
            } else {
                0.0
            }
        });
        feature_set.add_feature(Rc::new(Feature::new(
            evaluate,
            "prevWordAndCat",
            parameters,
        )));
    }

    Ok(())
}

/// One `{ token, tag }` pair.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaggedWord {
    /// The surface form.
    pub token: String,
    /// Its part-of-speech tag.
    pub tag: String,
}

impl TaggedWord {
    /// A tagged word.
    pub fn new(token: impl Into<String>, tag: impl Into<String>) -> Self {
        Self {
            token: token.into(),
            tag: tag.into(),
        }
    }
}

/// A tagged sentence that can emit maximum-entropy sample elements.
#[derive(Debug, Clone, Default)]
pub struct MESentence {
    /// The tagged words, in order.
    pub tagged_words: Vec<TaggedWord>,
}

impl MESentence {
    /// An empty sentence.
    pub fn new() -> Self {
        Self::default()
    }

    /// A sentence over the given tagged words.
    pub fn with_tagged_words(tagged_words: Vec<TaggedWord>) -> Self {
        Self { tagged_words }
    }

    /// `addTaggedWord(token, tag)`.
    pub fn add_tagged_word(&mut self, token: impl Into<String>, tag: impl Into<String>) {
        self.tagged_words.push(TaggedWord::new(token, tag));
    }

    /// `generateSampleElements(sample)`: one element per token.
    ///
    /// The windows are populated in the reference's own insertion order — `0`,
    /// then `-2`, `-1`, `1`, `2` — which is invisible in the context *key*
    /// (`safe-stable-stringify` sorts) but visible in `save()` output, where
    /// the reference hoists the array-index keys `0`, `1`, `2` ahead of `-2` and
    /// `-1`.
    pub fn generate_sample_elements(&self, sample: &mut Sample) {
        let len = self.tagged_words.len();
        for (index, token) in self.tagged_words.iter().enumerate() {
            let mut word_window: Vec<(String, DynValue)> = Vec::new();
            let mut tag_window: Vec<(String, DynValue)> = Vec::new();

            word_window.push(("0".to_owned(), DynValue::Str(token.token.clone())));
            tag_window.push(("0".to_owned(), DynValue::Str(token.tag.clone())));

            if index > 1 {
                let w = &self.tagged_words[index - 2];
                tag_window.push(("-2".to_owned(), DynValue::Str(w.tag.clone())));
                word_window.push(("-2".to_owned(), DynValue::Str(w.token.clone())));
            }
            if index > 0 {
                let w = &self.tagged_words[index - 1];
                tag_window.push(("-1".to_owned(), DynValue::Str(w.tag.clone())));
                word_window.push(("-1".to_owned(), DynValue::Str(w.token.clone())));
            }
            if index + 1 < len {
                let w = &self.tagged_words[index + 1];
                tag_window.push(("1".to_owned(), DynValue::Str(w.tag.clone())));
                word_window.push(("1".to_owned(), DynValue::Str(w.token.clone())));
            }
            if index + 2 < len {
                let w = &self.tagged_words[index + 2];
                tag_window.push(("2".to_owned(), DynValue::Str(w.tag.clone())));
                word_window.push(("2".to_owned(), DynValue::Str(w.token.clone())));
            }

            let data = DynValue::Obj(vec![
                ("wordWindow".to_owned(), DynValue::Obj(word_window)),
                ("tagWindow".to_owned(), DynValue::Obj(tag_window)),
            ]);
            sample.add_element(Rc::new(POSElement::new(
                token.tag.clone(),
                Rc::new(Context::new(data)),
            )));
        }
    }
}

/// A corpus of tagged sentences.
#[derive(Debug, Clone, Default)]
pub struct MECorpus {
    /// The sentences.
    pub sentences: Vec<MESentence>,
    /// Words counted while parsing. Note that `splitInTrainAndTest` does not
    /// maintain it, so both halves of a split report zero.
    pub word_count: usize,
}

impl MECorpus {
    /// An empty corpus.
    pub fn new() -> Self {
        Self::default()
    }

    /// The `typeOfCorpus === 2` path: a corpus given as tagged sentences.
    pub fn from_tagged(sentences: Vec<Vec<TaggedWord>>) -> Self {
        let mut out = Self::new();
        for words in sentences {
            out.word_count += words.len();
            out.sentences.push(MESentence::with_tagged_words(words));
        }
        out
    }

    /// The `typeOfCorpus === 1` path: BROWN-format `token_TAG` text.
    ///
    /// Lines that trim to nothing are skipped; the rest are split on runs of
    /// the reference `\s`, and each token on its first `_`. A token with no
    /// underscore yields the tag `"undefined"`, which is what the reference
    /// stringifies the missing element to when it becomes a class name.
    pub fn from_brown(data: &str) -> Self {
        let mut out = Self::new();
        for line in data.split('\n') {
            let trimmed = line.trim_matches(is_whitespace);
            if trimmed.is_empty() {
                continue;
            }
            let mut sentence = MESentence::new();
            for token in trimmed.split(is_whitespace).filter(|t| !t.is_empty()) {
                out.word_count += 1;
                let mut parts = token.split('_');
                let word = parts.next().unwrap_or_default();
                let tag = parts.next().unwrap_or("undefined");
                sentence.add_tagged_word(word, tag);
            }
            out.sentences.push(sentence);
        }
        out
    }

    /// `nrSentences()`.
    pub fn nr_sentences(&self) -> usize {
        self.sentences.len()
    }

    /// `nrWords()`.
    pub fn nr_words(&self) -> usize {
        self.word_count
    }

    /// `generateSample()`.
    ///
    /// Built with `new Sample([])`, the only constructor form that does not
    /// throw.
    pub fn generate_sample(&self) -> Sample {
        let mut sample = Sample::new();
        for sentence in &self.sentences {
            sentence.generate_sample_elements(&mut sample);
        }
        sample
    }

    /// `splitInTrainAndTest(percentageTrain)`, with the randomness supplied.
    ///
    /// The reference calls `Math.random()`, making the split non-deterministic
    /// and untestable; `random` stands in for it so a caller can choose. A
    /// `percentage_train` of 100 sends everything to the training half in both,
    /// since `Math.random() < 1` always holds.
    ///
    /// Neither returned corpus has a maintained `word_count` — the reference
    /// leaves both at zero, and so does this.
    pub fn split_in_train_and_test_with(
        &self,
        percentage_train: f64,
        mut random: impl FnMut() -> f64,
    ) -> (Self, Self) {
        let p = percentage_train / 100.0;
        let mut train = Self::new();
        let mut test = Self::new();
        for sentence in &self.sentences {
            if random() < p {
                train.sentences.push(sentence.clone());
            } else {
                test.sentences.push(sentence.clone());
            }
        }
        (train, test)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dog_sentence() -> MESentence {
        MESentence::with_tagged_words(vec![
            TaggedWord::new("the", "DT"),
            TaggedWord::new("big", "JJ"),
            TaggedWord::new("dog", "NN"),
            TaggedWord::new("runs", "VB"),
        ])
    }

    #[test]
    fn element_keys_match_the_recorded_reference_values() {
        let mut sample = Sample::new();
        dog_sentence().generate_sample_elements(&mut sample);
        let keys: Vec<String> = sample.elements().iter().map(|e| e.to_key()).collect();
        assert_eq!(
            keys,
            vec![
                r#"DT{"tagWindow":{"0":"DT","1":"JJ","2":"NN"},"wordWindow":{"0":"the","1":"big","2":"dog"}}"#,
                r#"JJ{"tagWindow":{"-1":"DT","0":"JJ","1":"NN","2":"VB"},"wordWindow":{"-1":"the","0":"big","1":"dog","2":"runs"}}"#,
                r#"NN{"tagWindow":{"-1":"JJ","-2":"DT","0":"NN","1":"VB"},"wordWindow":{"-1":"big","-2":"the","0":"dog","1":"runs"}}"#,
                r#"VB{"tagWindow":{"-1":"NN","-2":"JJ","0":"VB"},"wordWindow":{"-1":"dog","-2":"big","0":"runs"}}"#,
            ]
        );
    }

    #[test]
    fn features_are_generated_in_the_recorded_order() {
        let mut sample = Sample::new();
        dog_sentence().generate_sample_elements(&mut sample);
        let mut fs = FeatureSet::new();
        sample.generate_features(&mut fs).unwrap();
        assert_eq!(
            fs.pretty_print(),
            "wordFeature | 0|the|0|DT\n\
             wordFeature | 0|big|0|JJ\n\
             prevWordAndCat | 0|JJ|-1|the|-1|DT\n\
             wordFeature | 0|dog|0|NN\n\
             prevBigram | 0|NN|-2|DT|-1|JJ\n\
             prevWordAndCat | 0|NN|-1|big|-1|JJ\n\
             wordFeature | 0|runs|0|VB\n\
             prevBigram | 0|VB|-2|JJ|-1|NN\n\
             prevWordAndCat | 0|VB|-1|dog|-1|NN\n"
        );
    }

    #[test]
    fn brown_parsing_skips_blank_lines() {
        let c = MECorpus::from_brown("the_DT big_JJ\n\n   \n a_DT\tb_NN   c_JJ\n");
        assert_eq!(c.nr_sentences(), 2);
        assert_eq!(c.word_count, 5);
        assert_eq!(c.sentences[1].tagged_words[2], TaggedWord::new("c", "JJ"));
        assert_eq!(MECorpus::from_brown("").nr_sentences(), 0);
    }

    #[test]
    fn a_split_at_a_hundred_percent_keeps_everything_and_forgets_the_word_count() {
        let c = MECorpus::from_tagged(vec![
            vec![TaggedWord::new("a", "DT")],
            vec![TaggedWord::new("b", "NN")],
        ]);
        let (train, test) = c.split_in_train_and_test_with(100.0, || 0.5);
        assert_eq!(train.nr_sentences(), 2);
        assert_eq!(test.nr_sentences(), 0);
        assert_eq!(train.word_count, 0);
    }

    #[test]
    fn a_context_without_windows_is_rejected() {
        let mut fs = FeatureSet::new();
        assert_eq!(
            POSElement::new("DT", Rc::new(Context::of_str("plain"))).generate_features(&mut fs),
            Err(MaxEntError::PosContextMissingWindows)
        );
    }

    #[test]
    fn empty_window_entries_skip_their_features() {
        // wordWindow['-1'] === '' is falsy, so prevWordAndCat is not added,
        // while a truthy tagWindow['-2'] still adds prevBigram.
        let data = DynValue::Obj(vec![
            (
                "wordWindow".to_owned(),
                DynValue::Obj(vec![
                    ("0".to_owned(), DynValue::Str("w".into())),
                    ("-1".to_owned(), DynValue::Str(String::new())),
                ]),
            ),
            (
                "tagWindow".to_owned(),
                DynValue::Obj(vec![
                    ("0".to_owned(), DynValue::Str("T".into())),
                    ("-2".to_owned(), DynValue::Str("Y".into())),
                    ("-1".to_owned(), DynValue::Str("Z".into())),
                ]),
            ),
        ]);
        let mut fs = FeatureSet::new();
        POSElement::new("T", Rc::new(Context::new(data)))
            .generate_features(&mut fs)
            .unwrap();
        assert_eq!(fs.size(), 2);
        assert!(fs.pretty_print().contains("prevBigram"));
        assert!(!fs.pretty_print().contains("prevWordAndCat"));
    }
}
