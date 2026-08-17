//! The tagger itself: lexicon lookup, then transformation rules.
//!
//! # Two levels, one behaviour
//!
//! [`BrillPosTagger::tag_iter`] is the primitive. It pulls tokens from any
//! iterator and yields tagged words one at a time, holding at most seven of them
//! in a fixed window, so a document of any size tags in constant memory and
//! nothing is copied — tokens are `&str` slices into the caller's input.
//! [`BrillPosTagger::tag`] and [`BrillPosTagger::apply_rules`] are the
//! The reference-shaped conveniences, and both route through the same per-position
//! routine, so there is exactly one description of what a rule does.
//!
//! # Why streaming is exact, not an approximation
//!
//! `applyRules` walks positions left to right, running **every** rule at each
//! one and mutating tags in place. A rule at position *i* therefore sees:
//!
//! * positions *i-1*, *i-2*, *i-3* — already **final**;
//! * position *i* — as edited by rules earlier in the same rule set;
//! * positions *i+1*, *i+2*, *i+3* — still carrying their **original lexicon**
//!   tags, because those positions have not been visited yet.
//!
//! No predicate in the closed 36-template set reaches further than three
//! positions either way, so a window of three behind and three ahead reproduces
//! the batch result exactly — including the bounds tests, since "fewer than
//! three exist ahead" is equally true of the window and of the whole sentence.

use std::borrow::Cow;

#[cfg(feature = "parallel")]
use rayon::prelude::*;

use crate::error::TaggerError;
use crate::lexicon::Lexicon;
use crate::rule::TransformationRule;
use crate::ruleset::RuleSet;
use crate::sentence::{Sentence, TaggedWord};

/// How far a predicate can look in each direction: `PREV1OR2OR3TAG` and
/// `NEXT1OR2OR3TAG` reach three positions, and nothing reaches further.
const REACH: usize = 3;

/// Applies every rule of `rules`, in order, at one position.
///
/// This is the single source of rule behaviour: both the batch and the streaming
/// path call it, with `words` being either the whole sentence or the window.
///
/// # Errors
///
/// Propagates whatever a predicate throws.
#[inline]
pub fn apply_rules_at(
    rules: &[TransformationRule],
    words: &mut [TaggedWord<'_>],
    position: isize,
) -> Result<(), TaggerError> {
    for rule in rules {
        rule.apply(words, position)?;
    }
    Ok(())
}

/// A Brill part-of-speech tagger: a lexicon plus a rule set.
///
/// Both are borrowed, as they are in the reference, so changes to either — an
/// `add_word`, an added rule — are visible to the tagger immediately.
#[derive(Debug, Clone, Copy)]
pub struct BrillPosTagger<'a> {
    /// The dictionary used for the initial tagging.
    pub lexicon: &'a Lexicon,
    /// The transformation rules applied afterwards.
    pub rule_set: &'a RuleSet,
}

impl<'a> BrillPosTagger<'a> {
    /// Builds a tagger over a lexicon and rule set.
    #[must_use]
    pub const fn new(lexicon: &'a Lexicon, rule_set: &'a RuleSet) -> Self {
        Self { lexicon, rule_set }
    }

    /// `tagWithLexicon(sentence)`: the initial tagging, before any rule runs.
    ///
    /// Each word takes `lexicon.tagWord(word)[0]`, **unconditionally** — a word
    /// present with an empty category list yields no tag rather than the default
    /// category.
    pub fn tag_with_lexicon<'t, I>(&self, words: I) -> Sentence<'t>
    where
        I: IntoIterator,
        I::Item: Into<Cow<'t, str>>,
    {
        let iter = words.into_iter();
        let mut sentence = Sentence {
            tagged_words: Vec::with_capacity(iter.size_hint().0),
        };
        for w in iter {
            let token: Cow<'t, str> = w.into();
            let tag = self.lexicon.first_category(&token);
            sentence.tagged_words.push(TaggedWord::new(token, tag));
        }
        sentence
    }

    /// `applyRules(sentence)`: runs every rule at every position, in place.
    ///
    /// # Errors
    ///
    /// Propagates predicate errors — for example an empty token reaching
    /// `CURRENT-WORD-IS-CAP`. Neither bundled rule set can fail this way.
    pub fn apply_rules(&self, sentence: &mut Sentence<'_>) -> Result<(), TaggerError> {
        let rules = self.rule_set.rules();
        let words = &mut sentence.tagged_words;
        for i in 0..words.len() {
            apply_rules_at(rules, words, i as isize)?;
        }
        Ok(())
    }

    /// `tag(sentence)`: lexicon lookup followed by the rules.
    ///
    /// # Errors
    ///
    /// Propagates predicate errors from the rule pass.
    pub fn tag<'t, I>(&self, words: I) -> Result<Sentence<'t>, TaggerError>
    where
        I: IntoIterator,
        I::Item: Into<Cow<'t, str>>,
    {
        let mut sentence = self.tag_with_lexicon(words);
        self.apply_rules(&mut sentence)?;
        Ok(sentence)
    }

    /// Tags a token stream lazily, in constant memory.
    ///
    /// The result of collecting this iterator is identical to [`Self::tag`]; see
    /// the module documentation for why a seven-word window suffices.
    ///
    /// ```
    /// use verbora_tagger::{BrillPosTagger, Language, Lexicon, RuleSet};
    ///
    /// let lexicon = Lexicon::detached(Some("EN"), Some("NN"), None);
    /// let rules = RuleSet::for_language(Language::English);
    /// let tagger = BrillPosTagger::new(&lexicon, &rules);
    ///
    /// let tags: Vec<_> = tagger
    ///     .tag_iter("the dog runs quickly".split(' '))
    ///     .map(|w| w.unwrap().tag().map(str::to_string))
    ///     .collect();
    /// assert_eq!(tags[3].as_deref(), Some("RB"));
    /// ```
    pub fn tag_iter<'t, I>(&self, tokens: I) -> TagIter<'a, 't, I::IntoIter>
    where
        I: IntoIterator,
        I::Item: Into<Cow<'t, str>>,
    {
        TagIter {
            tagger: *self,
            src: tokens.into_iter(),
            buf: Vec::with_capacity(2 * REACH + 2),
            cursor: 0,
            src_done: false,
            failed: false,
        }
    }

    /// Tags many independent documents in parallel, one [`Self::tag`] call per
    /// document, distributed across a Rayon thread pool.
    ///
    /// # When to reach for this over [`Self::tag`] in a loop
    ///
    /// Only when tagging **many separate documents** that do not depend on one
    /// another — a batch job, a corpus being tagged offline, a request handler
    /// fanning out over an uploaded set of files. `lexicon` and `rule_set` are
    /// read-only for the whole batch (`tag` never mutates either), so sharing
    /// `&self` across worker threads is safe with no locking and no per-document
    /// setup cost.
    ///
    /// Do **not** reach for this to parallelise *within* one document: `tag`
    /// already runs its rule pass in O(positions × rules) single-threaded work
    /// with no independent chunks to split, and [`Self::tag_iter`] is the right
    /// tool when memory, not throughput, is the constraint.
    ///
    /// # What it costs
    ///
    /// This crate's own bench (`tag/english`) measures roughly 200us for a
    /// 512-token document and 846us for a 4096-token one — near-linear in
    /// document length. That is comfortably above Rayon's per-task dispatch
    /// overhead, so on typical multi-core hardware `par_tag_batch` measures
    /// faster than the sequential loop even at a handful of documents, and the
    /// margin grows with both document count and core count: the
    /// `tag_batch/english-512tok` Criterion group in `benches/tagger.rs`
    /// measured (32-core host, 512 tokens/document) roughly 153us→104us at 1
    /// document, 885us→410us at 8 (~2.2×), 6.9ms→1.4ms at 64 (~5×), and
    /// 33ms→5.5ms at 256 (~6×). Documents much shorter than a few hundred
    /// tokens, or a build with only one or two cores available, narrow or
    /// remove that margin — re-run the bench group on the target hardware
    /// before assuming a number.
    ///
    /// This allocates one `Vec` for the results (exactly as a sequential
    /// `documents.iter().map(tag).collect()` would) plus whatever each `tag`
    /// call itself allocates; no additional buffering happens per document.
    ///
    /// Output order always matches input order, and each element is exactly
    /// what the equivalent sequential call would have produced — including
    /// which documents come back `Err`. This is deliberately *not* a new
    /// algorithm: the body is a `par_iter().map(Self::tag).collect()` over the
    /// same per-document routine used everywhere else in this module.
    ///
    /// # Errors
    ///
    /// Each element of the returned `Vec` carries its own `Result`, identical
    /// to calling [`Self::tag`] on that document alone — one document failing
    /// its rule pass does not affect any other document's result.
    #[cfg(feature = "parallel")]
    pub fn par_tag_batch<'t, D>(&self, documents: &[D]) -> Vec<Result<Sentence<'t>, TaggerError>>
    where
        D: AsRef<[&'t str]> + Sync,
    {
        documents
            .par_iter()
            .map(|doc| self.tag(doc.as_ref().iter().copied()))
            .collect()
    }
}

/// Lazy tagger output; see [`BrillPosTagger::tag_iter`].
#[derive(Debug)]
pub struct TagIter<'a, 't, I> {
    tagger: BrillPosTagger<'a>,
    src: I,
    /// The live window: finalised history, the position being worked on, and the
    /// lookahead. Never longer than `2 * REACH + 1`.
    buf: Vec<TaggedWord<'t>>,
    /// Index in `buf` of the next position to run rules at.
    cursor: usize,
    src_done: bool,
    failed: bool,
}

impl<'t, I, T> Iterator for TagIter<'_, 't, I>
where
    I: Iterator<Item = T>,
    T: Into<Cow<'t, str>>,
{
    type Item = Result<TaggedWord<'t>, TaggerError>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.failed {
            return None;
        }
        loop {
            // Keep REACH words of lookahead beyond the cursor, so the bounds
            // tests inside the predicates see the same answers they would on the
            // whole sentence.
            while !self.src_done && self.buf.len() < self.cursor + REACH + 1 {
                match self.src.next() {
                    Some(tok) => {
                        let token: Cow<'t, str> = tok.into();
                        let tag = self.tagger.lexicon.first_category(&token);
                        self.buf.push(TaggedWord::new(token, tag));
                    }
                    None => self.src_done = true,
                }
            }

            // The front word is emitted once no future position can read it,
            // i.e. once REACH finalised words separate it from the cursor.
            let drained = self.src_done && self.cursor == self.buf.len();
            if self.cursor > REACH || (drained && !self.buf.is_empty()) {
                let w = self.buf.remove(0);
                self.cursor -= 1;
                return Some(Ok(w));
            }

            if self.cursor < self.buf.len() {
                let rules = self.tagger.rule_set.rules();
                let at = self.cursor as isize;
                if let Err(e) = apply_rules_at(rules, &mut self.buf, at) {
                    self.failed = true;
                    return Some(Err(e));
                }
                self.cursor += 1;
            } else {
                return None;
            }
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let buffered = self.buf.len();
        let (lo, hi) = self.src.size_hint();
        (lo + buffered, hi.and_then(|h| h.checked_add(buffered)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ruleset::Language;

    fn en() -> (Lexicon, RuleSet) {
        (
            Lexicon::detached(Some("EN"), Some("NN"), None),
            RuleSet::for_language(Language::English),
        )
    }

    fn tags(s: &Sentence<'_>) -> Vec<Option<String>> {
        s.tagged_words
            .iter()
            .map(|w| w.tag().map(str::to_string))
            .collect()
    }

    #[test]
    fn streaming_matches_batch_on_every_length() {
        let (lex, rules) = en();
        let tagger = BrillPosTagger::new(&lex, &rules);
        let words = [
            "the",
            "quick",
            "brown",
            "fox",
            "jumps",
            "over",
            "the",
            "lazy",
            "dog",
            "quickly",
            "5",
            "www.example.com",
            "would",
            "book",
            "cats",
            "sees",
        ];
        for n in 0..=words.len() {
            let slice = &words[..n];
            let batch = tagger.tag(slice.iter().copied()).unwrap();
            let streamed: Vec<TaggedWord<'_>> = tagger
                .tag_iter(slice.iter().copied())
                .map(Result::unwrap)
                .collect();
            assert_eq!(
                streamed, batch.tagged_words,
                "length {n} diverges between the streaming and batch paths"
            );
        }
    }

    #[test]
    fn suffix_rules_use_first_occurrence() {
        let (lex, rules) = en();
        let tagger = BrillPosTagger::new(&lex, &rules);
        let got = tagger
            .tag(["zzcats", "zzsees", "zzstring", "zzsinging", "zzss"])
            .unwrap();
        assert_eq!(
            tags(&got),
            [
                Some("NNS".into()),
                Some("NN".into()),
                Some("VBG".into()),
                Some("NN".into()),
                Some("NN".into())
            ]
        );
    }

    /// One token per script and character class the crate is expected to
    /// survive, tagged end to end.
    ///
    /// The interesting column is `NNP` versus `NN`: the capitalised default is
    /// gated on `/[A-Z]/` against the first UTF-16 code unit, which is **ASCII
    /// only** — so `Ålesund`, `Москва`, `Ελλάς` and `İstanbul` all take the
    /// ordinary default however capitalised they look. A port reaching for
    /// `char::is_uppercase` gets every one of those wrong.
    #[test]
    fn every_character_class_tags_without_panicking() {
        let lex = Lexicon::detached(Some("EN"), Some("NN"), Some("NNP"));
        let rules = RuleSet::for_language(Language::English);
        let tagger = BrillPosTagger::new(&lex, &rules);
        let long = "z".repeat(10_000);
        let cases: &[(&str, Option<&str>)] = &[
            ("", None),                       // present with an empty tag list
            ("z", Some("NN")),                // single char, lowercase
            ("Z", Some("NNP")),               // single char, ASCII uppercase
            ("Zzzz", Some("NNP")),            // uppercase word
            ("café", Some("NN")),             // accented Latin
            ("Café", Some("NNP")),            // accented Latin, ASCII-capitalised
            ("Ålesund", Some("NN")),          // non-ASCII capital: NOT NNP
            ("Москва", Some("NN")),           // Cyrillic
            ("Ελλάς", Some("NN")),            // Greek
            ("ΟΔΟΣ", Some("NN")),             // Greek, all caps
            ("İstanbul", Some("NN")),         // Turkish dotted capital
            ("straße", Some("NN")),           // sharp s
            ("日本語", Some("NN")),           // CJK
            ("😀", Some("NN")),               // astral
            ("𝐀bc", Some("NN")),              // astral, mathematically capital
            (".", Some(".")),                 // punctuation: tagged with itself
            ("5", Some("CD")),                // digits
            ("3.14", Some("CD")),             // decimal: a URL rule could steal this
            ("www.example.com", Some("URL")), // two adjacent letters plus a dot
            (&long, Some("NN")),              // very long input
        ];
        for (token, want) in cases {
            let got = tagger.tag([*token]).unwrap();
            assert_eq!(got.tagged_words[0].tag(), *want, "{token:?}");
            assert_eq!(&*got.tagged_words[0].token, *token, "token was rewritten");
        }
    }

    /// A `Lexicon` aliases a process-global dictionary behind a lock, so it must
    /// be shareable; a regression here would make the crate single-threaded.
    #[test]
    fn taggers_and_lexicons_cross_threads() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<Lexicon>();
        assert_send_sync::<RuleSet>();
        assert_send_sync::<BrillPosTagger<'_>>();
        assert_send_sync::<Sentence<'_>>();
    }

    #[test]
    fn empty_and_single_token_sentences() {
        let (lex, rules) = en();
        let tagger = BrillPosTagger::new(&lex, &rules);
        assert!(tagger.tag(Vec::<&str>::new()).unwrap().is_empty());
        // The English lexicon has the key "" with an empty tag list, so the
        // token survives with NO tag rather than taking the default.
        let got = tagger.tag([""]).unwrap();
        assert_eq!(tags(&got), [None]);
        assert_eq!(tags(&tagger.tag(["a"]).unwrap()), [Some("DT".into())]);
    }

    #[test]
    fn rule_order_at_a_position_is_observable() {
        let lex = Lexicon::detached(Some("EN"), Some("NN"), None);
        let sentence = || {
            Sentence::from_words(vec![
                TaggedWord::new("the", Some(Cow::Borrowed("DT"))),
                TaggedWord::new("a", Some(Cow::Borrowed("NN"))),
                TaggedWord::new("b", Some(Cow::Borrowed("NN"))),
            ])
        };
        let forward =
            RuleSet::from_rule_strings(&["NN VB PREV-TAG DT", "NN JJ NEXT-TAG NN"]).unwrap();
        let mut s = sentence();
        BrillPosTagger::new(&lex, &forward)
            .apply_rules(&mut s)
            .unwrap();
        assert_eq!(
            tags(&s),
            [Some("DT".into()), Some("VB".into()), Some("NN".into())]
        );

        let reversed =
            RuleSet::from_rule_strings(&["NN JJ NEXT-TAG NN", "NN VB PREV-TAG DT"]).unwrap();
        let mut s = sentence();
        BrillPosTagger::new(&lex, &reversed)
            .apply_rules(&mut s)
            .unwrap();
        assert_eq!(
            tags(&s),
            [Some("DT".into()), Some("JJ".into()), Some("NN".into())]
        );
    }

    #[test]
    fn backward_predicates_see_final_tags_forward_ones_see_lexicon_tags() {
        // Position 1's rule fires on position 0's ALREADY-REWRITTEN tag, while
        // position 0's rule saw position 1's original tag. Any implementation
        // that pre-computed all rule matches would get this wrong.
        let lex = Lexicon::detached(Some("EN"), Some("NN"), None);
        let rules = RuleSet::from_rule_strings(&["AA BB CURWD x", "CC DD PREV-TAG BB"]).unwrap();
        let mut s = Sentence::from_words(vec![
            TaggedWord::new("x", Some(Cow::Borrowed("AA"))),
            TaggedWord::new("y", Some(Cow::Borrowed("CC"))),
        ]);
        BrillPosTagger::new(&lex, &rules)
            .apply_rules(&mut s)
            .unwrap();
        assert_eq!(tags(&s), [Some("BB".into()), Some("DD".into())]);
    }

    #[test]
    fn streaming_stops_after_an_error() {
        let lex = Lexicon::detached(Some("EN"), Some("NN"), None);
        // The wildcard old category, because "" is a real English entry with an
        // EMPTY tag list — so the token carries no tag for "NN" to match.
        let rules = RuleSet::from_rule_strings(&["* JJ CURRENT-WORD-IS-CAP YES"]).unwrap();
        let tagger = BrillPosTagger::new(&lex, &rules);
        let mut it = tagger.tag_iter(["ok", "", "more"]);
        // "" reaches currentWordIsCap, which reads `word[0].toUpperCase()`.
        let outcomes: Vec<_> = std::iter::from_fn(|| it.next()).collect();
        assert!(outcomes.iter().any(Result::is_err));
        assert_eq!(
            tagger.tag(["ok", "", "more"]),
            Err(TaggerError::undefined("toUpperCase"))
        );
    }

    #[test]
    fn very_long_input_streams_in_constant_memory() {
        let (lex, rules) = en();
        let tagger = BrillPosTagger::new(&lex, &rules);
        let n = 50_000;
        let count = tagger
            .tag_iter(std::iter::repeat_n("dog", n))
            .filter(|w| w.as_ref().is_ok_and(|w| w.tag() == Some("NN")))
            .count();
        assert_eq!(count, n);
    }

    /// `par_tag_batch` must reproduce `tag` called once per document, in
    /// order, including on inputs this crate's own suite already knows are
    /// tricky: the empty document, a single token, the mixed-length sentence
    /// from [`streaming_matches_batch_on_every_length`], and the full
    /// character-class sweep from
    /// [`every_character_class_tags_without_panicking`].
    #[cfg(feature = "parallel")]
    #[test]
    fn par_tag_batch_matches_sequential_tag_per_document() {
        let (lex, rules) = en();
        let tagger = BrillPosTagger::new(&lex, &rules);

        let documents: Vec<Vec<&str>> = vec![
            vec![],
            vec!["a"],
            vec![
                "the",
                "quick",
                "brown",
                "fox",
                "jumps",
                "over",
                "the",
                "lazy",
                "dog",
                "quickly",
                "5",
                "www.example.com",
                "would",
                "book",
                "cats",
                "sees",
            ],
            vec![
                "",
                "z",
                "Z",
                "Zzzz",
                "café",
                "Café",
                "Ålesund",
                "Москва",
                "Ελλάς",
                "ΟΔΟΣ",
                "İstanbul",
                "straße",
                "日本語",
                "😀",
                "𝐀bc",
                ".",
                "5",
                "3.14",
                "www.example.com",
            ],
        ];

        let sequential: Vec<_> = documents
            .iter()
            .map(|doc| tagger.tag(doc.iter().copied()))
            .collect();
        let parallel = tagger.par_tag_batch(&documents);

        assert_eq!(parallel, sequential);
    }

    /// Each document's `Result` must survive the batch independently: one
    /// document's predicate error must not affect any other document's
    /// outcome, and the error must be byte-identical to the sequential one —
    /// the same case [`streaming_stops_after_an_error`] exercises.
    #[cfg(feature = "parallel")]
    #[test]
    fn par_tag_batch_preserves_per_document_errors() {
        let lex = Lexicon::detached(Some("EN"), Some("NN"), None);
        let rules = RuleSet::from_rule_strings(&["* JJ CURRENT-WORD-IS-CAP YES"]).unwrap();
        let tagger = BrillPosTagger::new(&lex, &rules);

        let documents: Vec<Vec<&str>> = vec![
            vec!["ok", "more"],
            vec!["ok", "", "more"],
            vec![],
            vec!["ok", "", "more"],
            vec!["fine"],
        ];

        let sequential: Vec<_> = documents
            .iter()
            .map(|doc| tagger.tag(doc.iter().copied()))
            .collect();
        let parallel = tagger.par_tag_batch(&documents);

        assert_eq!(parallel, sequential);
        assert!(sequential[0].is_ok());
        assert!(sequential[1].is_err());
        assert!(sequential[2].is_ok());
        assert!(sequential[3].is_err());
        assert!(sequential[4].is_ok());
    }
}
