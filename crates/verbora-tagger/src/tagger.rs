//! The tagger: initial-state annotation, then transformation rules.

use std::borrow::Cow;

#[cfg(feature = "parallel")]
use rayon::prelude::*;

use crate::corpus::Corpus;
use crate::lexicon::Lexicon;
use crate::ruleset::RuleSet;
use crate::tag::TaggedToken;

/// A Brill part-of-speech tagger: a [`Lexicon`] and a [`RuleSet`].
///
/// Both are borrowed, so one lexicon and one rule set can back any number of
/// taggers on any number of threads without copying either.
///
/// # How tagging works
///
/// 1. **Initial-state annotation.** Every token takes [`Lexicon::tag_of`] — its
///    most frequent tag, or a default. Brill (1995) §2.
/// 2. **Transformation.** Each rule of the set is applied, in order, to the
///    whole sentence before the next rule runs.
///
/// # Simultaneous application, stated on purpose
///
/// Within one rule, every site is decided against the tagging **as it stood
/// before that rule ran**, and all the resulting rewrites land together. The
/// alternative — rewriting in place as a left-to-right scan proceeds — makes a
/// rule's own earlier rewrites visible to its own later tests, so the result
/// depends on scan direction and a position's outcome can depend on the entire
/// prefix of the sentence. Verbora chooses simultaneous application because it
/// keeps a transformation's effect a function of a bounded window, which is what
/// makes [`BrillTagger::tag_stream`] exact rather than approximate.
///
/// # Nothing is rewritten but the tag
///
/// Tokens come back byte-identical to the ones handed in, borrowed from the
/// caller's input where possible.
///
/// # Choosing the right API
///
/// | API | Use when | Allocates | Returns |
/// |---|---|---|---|
/// | [`tag`](Self::tag) | the default; a sentence or document already in memory | one `Vec` of the output length | `Vec<TaggedToken>` |
/// | [`tag_into`](Self::tag_into) | tagging many documents in a loop, reusing one buffer | nothing, once the buffer is warm | appends to your `Vec` |
/// | [`tag_stream`](Self::tag_stream) | input arrives lazily, or is far larger than memory | `O(block + context)`, independent of input length | an `Iterator` |
/// | [`annotate`](Self::annotate) | you want the lexicon's answer *without* the rules | one `Vec` | `Vec<TaggedToken>` |
/// | [`transform`](Self::transform) | you already hold tagged tokens and want the rules re-run | one scratch `Vec<usize>` | in place |
/// | [`par_tag_batch`](Self::par_tag_batch) | **many independent documents**, `parallel` feature on | one `Vec` per document | `Vec<Vec<TaggedToken>>` |
///
/// The decision is short:
///
/// * One document in memory → [`tag`](Self::tag). It is the right choice for
///   the large majority of programs; the others exist for shapes it handles
///   badly, not because it is slow.
/// * A loop over documents where the tagged output is consumed and discarded
///   each time → [`tag_into`](Self::tag_into), which reuses the allocation
///   [`tag`](Self::tag) would make and free once per document.
/// * A document that does not fit, or a token source that is itself lazy →
///   [`tag_stream`](Self::tag_stream). It buffers
///   `RuleSet::context_span` tokens on each side plus a block, so its
///   memory is bounded by the *rule set*, not by the document. It costs one
///   clone of each buffered token, so it is not the faster option for input
///   that already fits.
/// * Many documents and more than one core → [`par_tag_batch`](Self::par_tag_batch),
///   whose body is `documents.par_iter().map(tag).collect()` and nothing more.
///
/// Every one of these produces the same tags; they differ only in where the
/// memory goes. `tests/api_equivalence.rs` asserts that over the crate's own
/// character-class sweep.
///
/// Performance for the post-migration implementation is **unmeasured**: the
/// crate's benchmarks compile and are ready to run, but no campaign has been run
/// against this code, so no timing figure is published here.
#[derive(Debug, Clone, Copy)]
pub struct BrillTagger<'a> {
    /// The dictionary used for the initial tagging.
    pub lexicon: &'a Lexicon,
    /// The transformation rules applied afterwards, in order.
    pub rules: &'a RuleSet,
}

/// How many positions [`BrillTagger::tag_stream`] finalises per refill, before
/// context is added on each side.
const STREAM_BLOCK: usize = 1024;

impl<'a> BrillTagger<'a> {
    /// Builds a tagger over a lexicon and a rule set.
    #[must_use]
    pub const fn new(lexicon: &'a Lexicon, rules: &'a RuleSet) -> Self {
        Self { lexicon, rules }
    }

    /// The initial state: every token takes [`Lexicon::tag_of`], no rules.
    ///
    /// Useful on its own for measuring how much the rules contribute — that is
    /// what [`Evaluation::accuracy_before_rules`] reports.
    pub fn annotate<'t, I>(&self, tokens: I) -> Vec<TaggedToken<'t>>
    where
        I: IntoIterator,
        I::Item: Into<Cow<'t, str>>,
    {
        let mut out = Vec::new();
        self.annotate_into(tokens, &mut out);
        out
    }

    /// [`Self::annotate`], appending to `out` instead of allocating.
    pub fn annotate_into<'t, I>(&self, tokens: I, out: &mut Vec<TaggedToken<'t>>)
    where
        I: IntoIterator,
        I::Item: Into<Cow<'t, str>>,
    {
        let iter = tokens.into_iter();
        out.reserve(iter.size_hint().0);
        for t in iter {
            let token: Cow<'t, str> = t.into();
            let tag = self.lexicon.tag_of(&token);
            out.push(TaggedToken { token, tag });
        }
    }

    /// Applies every rule, in order, to an already-tagged sentence, in place.
    ///
    /// Each rule sees the tagging the previous rule produced; within one rule,
    /// every site is decided before any rewrite lands.
    pub fn transform(&self, words: &mut [TaggedToken<'_>]) {
        let mut sites = Vec::new();
        self.transform_with(words, &mut sites);
    }

    /// [`Self::transform`] with a caller-supplied scratch buffer.
    ///
    /// The buffer holds the sites one rule fires at; reusing it across sentences
    /// removes the one allocation `transform` makes. It is cleared on entry, so
    /// forgetting to clear it is not a correctness bug.
    pub fn transform_with(&self, words: &mut [TaggedToken<'_>], sites: &mut Vec<usize>) {
        for rule in self.rules.rules() {
            sites.clear();
            sites.extend((0..words.len()).filter(|&i| rule.applies_at(words, i)));
            for &i in sites.iter() {
                words[i].tag = rule.to.clone();
            }
        }
    }

    /// Tags a sentence or document: initial state, then the rules.
    ///
    /// ```
    /// use verbora_tagger::{BrillTagger, Corpus, RuleSet, Tag};
    ///
    /// let corpus = Corpus::parse_brown(
    ///     "I_PRP would_MD read_VB a_DT book_NN\n\
    ///      the_DT book_NN was_VBD good_JJ",
    /// )?;
    /// let lexicon = corpus.build_lexicon(Tag::new("NN")?)?;
    /// let rules: RuleSet = "NN VB PREV-TAG MD".parse()?;
    /// let tagger = BrillTagger::new(&lexicon, &rules);
    ///
    /// let tagged = tagger.tag(["I", "would", "book", "a", "flight"]);
    /// let tags: Vec<&str> = tagged.iter().map(|w| w.tag().as_str()).collect();
    /// assert_eq!(tags, ["PRP", "MD", "VB", "DT", "NN"]);
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    ///
    /// `book` is `NN` in the lexicon — the corpus above tags it that way twice
    /// and never as a verb — and the rule `NN VB PREV-TAG MD` is what makes it a
    /// verb here. `flight` is not in the lexicon at all, so it takes the default
    /// tag the lexicon was built with.
    pub fn tag<'t, I>(&self, tokens: I) -> Vec<TaggedToken<'t>>
    where
        I: IntoIterator,
        I::Item: Into<Cow<'t, str>>,
    {
        let mut out = Vec::new();
        self.tag_into(tokens, &mut out);
        out
    }

    /// [`Self::tag`], appending to `out` instead of allocating a fresh `Vec`.
    ///
    /// # Why it exists
    ///
    /// A loop that tags many documents and consumes each result immediately
    /// otherwise allocates and frees one `Vec` per document. `tag_into` lets one
    /// buffer serve the whole loop:
    ///
    /// ```
    /// # use verbora_tagger::{BrillTagger, Lexicon, RuleSet, Tag};
    /// # let mut lexicon = Lexicon::new(Tag::new("NN").unwrap());
    /// # lexicon.insert("the", vec![Tag::new("DT").unwrap()]).unwrap();
    /// # let rules = RuleSet::new();
    /// # let tagger = BrillTagger::new(&lexicon, &rules);
    /// # let documents = [vec!["the", "dog"], vec!["a", "cat"]];
    /// let mut buffer = Vec::new();
    /// for document in &documents {
    ///     buffer.clear();
    ///     tagger.tag_into(document.iter().copied(), &mut buffer);
    ///     assert!(!buffer.is_empty());
    /// }
    /// ```
    ///
    /// # What it costs
    ///
    /// The `clear()` is yours to remember — but forgetting it is *not* silently
    /// wrong here: the rules are applied only to the newly appended range, so a
    /// stale prefix is left exactly as it was rather than re-transformed. What
    /// you get is a growing buffer, not corrupted tags. Tokens already in `out`
    /// are also invisible to the new range's conditions, so appending twice is
    /// not the same as tagging the concatenation — which is the right semantics,
    /// since the two calls are two documents.
    pub fn tag_into<'t, I>(&self, tokens: I, out: &mut Vec<TaggedToken<'t>>)
    where
        I: IntoIterator,
        I::Item: Into<Cow<'t, str>>,
    {
        let start = out.len();
        self.annotate_into(tokens, out);
        self.transform(&mut out[start..]);
    }

    /// Tags a lazily produced token stream in memory bounded by the rule set.
    ///
    /// The output equals [`Self::tag`]'s, element for element, for any input.
    ///
    /// # Why it exists, and what it buffers
    ///
    /// Each rule runs over the whole sentence before the next one, so rule *k*'s
    /// answer at position *i* can depend on rule *k-1*'s answers up to
    /// `reach(rule_k)` away — and the reaches add up over the set. Summed, that
    /// is [`RuleSet::context_span`]. This iterator therefore holds
    /// `context_span.0 + 1024 + context_span.1` tokens at a time and no more,
    /// whatever the document length: it finalises 1024 positions per refill,
    /// with exactly enough context on each side for those positions to come out
    /// identical to a whole-document run.
    ///
    /// That is a property of the rule set, not of the input: ten rules that each
    /// read one token to the left buffer ten tokens of left context, whether the
    /// document is a sentence or a gigabyte.
    ///
    /// # What it costs
    ///
    /// Each buffered token is cloned once into the working block. For tokens
    /// borrowed from the caller's input (`&str`) that is a pointer-and-length
    /// copy; for owned `String` tokens it is a real copy. If the document
    /// already fits in memory, [`Self::tag`] does strictly less work.
    ///
    /// ```
    /// use verbora_tagger::{BrillTagger, Lexicon, RuleSet, Tag};
    ///
    /// let mut lexicon = Lexicon::new(Tag::new("NN")?);
    /// lexicon.insert("the", vec![Tag::new("DT")?])?;
    /// lexicon.insert("runs", vec![Tag::new("VBZ")?])?;
    /// // Every unknown token starts as `NN`; this rule moves the `-ly` ones.
    /// let rules: RuleSet = "NN RB CURRENT-WORD-ENDS-WITH ly".parse()?;
    /// let tagger = BrillTagger::new(&lexicon, &rules);
    ///
    /// let tags: Vec<String> = tagger
    ///     .tag_stream("the dog runs quickly".split(' '))
    ///     .map(|w| w.tag().to_string())
    ///     .collect();
    /// assert_eq!(tags, ["DT", "NN", "VBZ", "RB"]);
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    pub fn tag_stream<'t, I>(&self, tokens: I) -> TagStream<'a, 't, I::IntoIter>
    where
        I: IntoIterator,
        I::Item: Into<Cow<'t, str>>,
    {
        let (left, right) = self.rules.context_span();
        TagStream {
            tagger: *self,
            src: tokens.into_iter(),
            window: Vec::new(),
            base: 0,
            cursor: 0,
            left,
            right,
            ready: std::collections::VecDeque::new(),
            src_done: false,
        }
    }

    /// Tags many independent documents in parallel, one [`Self::tag`] call per
    /// document.
    ///
    /// # When to reach for it
    ///
    /// Only when tagging **many separate documents** that do not depend on one
    /// another. `lexicon` and `rules` are read-only for the whole batch, so
    /// sharing `&self` across worker threads needs no locking and no per-document
    /// setup.
    ///
    /// Do not reach for it to parallelise *within* one document: a document is a
    /// single ordered rule pass with no independent chunks to split, and
    /// [`Self::tag_stream`] is the tool when memory rather than throughput is the
    /// constraint.
    ///
    /// # What it costs
    ///
    /// One `Vec` per document, exactly as a sequential
    /// `documents.iter().map(tag).collect()` would allocate, plus Rayon's
    /// per-task dispatch. Output order always matches input order, and each
    /// element is what the equivalent sequential call would have produced.
    ///
    /// The crossover point is **unmeasured for this implementation** — the
    /// `tag_batch` bench group exists but no campaign has been run against the
    /// post-migration code, so no speedup figure is published. Measure on your
    /// own hardware before assuming one.
    #[cfg(feature = "parallel")]
    pub fn par_tag_batch<'t, D>(&self, documents: &[D]) -> Vec<Vec<TaggedToken<'t>>>
    where
        D: AsRef<[&'t str]> + Sync,
    {
        documents
            .par_iter()
            .map(|doc| self.tag(doc.as_ref().iter().copied()))
            .collect()
    }

    /// Scores the tagger against an annotated corpus.
    ///
    /// The corpus is only read. Each sentence is tagged from its tokens alone,
    /// so nothing leaks between sentences and the gold annotation is never
    /// touched.
    #[must_use]
    pub fn evaluate(&self, corpus: &Corpus<'_>) -> Evaluation {
        let mut ev = Evaluation::default();
        let mut buf: Vec<TaggedToken<'_>> = Vec::new();
        let mut sites = Vec::new();
        for sentence in corpus.sentences() {
            buf.clear();
            self.annotate_into(sentence.iter().map(|w| w.token()), &mut buf);
            ev.tokens += sentence.len();
            ev.correct_before_rules += sentence
                .iter()
                .zip(&buf)
                .filter(|(gold, got)| gold.tag == got.tag)
                .count();
            self.transform_with(&mut buf, &mut sites);
            ev.correct_after_rules += sentence
                .iter()
                .zip(&buf)
                .filter(|(gold, got)| gold.tag == got.tag)
                .count();
        }
        ev
    }
}

/// What [`BrillTagger::evaluate`] measured.
///
/// Counts, not percentages: an empty corpus has no accuracy, and reporting
/// `0/0` as a number would either invent a value or produce a `NaN` that
/// poisons every later comparison. The percentages are available from
/// [`Evaluation::accuracy`] and [`Evaluation::accuracy_before_rules`], both of
/// which return `None` for an empty corpus.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Evaluation {
    /// Tokens compared.
    pub tokens: usize,
    /// Tokens the lexicon alone tagged correctly.
    pub correct_before_rules: usize,
    /// Tokens tagged correctly after the rules ran.
    pub correct_after_rules: usize,
}

impl Evaluation {
    /// Fraction correct after the rules, in `0.0..=1.0`, or `None` for an empty
    /// corpus.
    ///
    /// The division is the only floating-point operation: the counters are
    /// exact integers, so there is no accumulation order to get wrong.
    #[must_use]
    pub fn accuracy(&self) -> Option<f64> {
        self.fraction(self.correct_after_rules)
    }

    /// Fraction correct from the lexicon alone, or `None` for an empty corpus.
    #[must_use]
    pub fn accuracy_before_rules(&self) -> Option<f64> {
        self.fraction(self.correct_before_rules)
    }

    #[allow(clippy::cast_precision_loss)] // counts stay far below 2^53
    fn fraction(&self, correct: usize) -> Option<f64> {
        (self.tokens > 0).then(|| correct as f64 / self.tokens as f64)
    }
}

/// Lazy tagger output in bounded memory; see [`BrillTagger::tag_stream`].
#[derive(Debug)]
pub struct TagStream<'a, 't, I> {
    tagger: BrillTagger<'a>,
    src: I,
    /// Initial-state tokens from absolute position `base` onwards.
    window: Vec<TaggedToken<'t>>,
    base: usize,
    /// Absolute position of the next token to emit.
    cursor: usize,
    left: usize,
    right: usize,
    ready: std::collections::VecDeque<TaggedToken<'t>>,
    src_done: bool,
}

impl<'t, I, T> TagStream<'_, 't, I>
where
    I: Iterator<Item = T>,
    T: Into<Cow<'t, str>>,
{
    /// Pulls from the source until `window` reaches absolute position `want`.
    fn fill_to(&mut self, want: usize) {
        while !self.src_done && self.base + self.window.len() < want {
            match self.src.next() {
                Some(t) => {
                    let token: Cow<'t, str> = t.into();
                    let tag = self.tagger.lexicon.tag_of(&token);
                    self.window.push(TaggedToken { token, tag });
                }
                None => self.src_done = true,
            }
        }
    }

    /// Finalises the next block of positions into `ready`.
    fn refill(&mut self) {
        let block_end = self.cursor + STREAM_BLOCK;
        self.fill_to(block_end + self.right);
        let end = self.base + self.window.len();
        if self.cursor >= end {
            return;
        }
        let emit_to = block_end.min(end);
        let lo = self.cursor.saturating_sub(self.left).max(self.base);
        let hi = (emit_to + self.right).min(end);

        let mut block: Vec<TaggedToken<'t>> = self.window[lo - self.base..hi - self.base].to_vec();
        self.tagger.transform(&mut block);
        self.ready
            .extend(block.drain(self.cursor - lo..emit_to - lo));
        self.cursor = emit_to;

        // Keep only the left context the next block will need.
        let keep_from = self.cursor.saturating_sub(self.left).max(self.base);
        self.window.drain(..keep_from - self.base);
        self.base = keep_from;
    }
}

impl<'t, I, T> Iterator for TagStream<'_, 't, I>
where
    I: Iterator<Item = T>,
    T: Into<Cow<'t, str>>,
{
    type Item = TaggedToken<'t>;

    fn next(&mut self) -> Option<TaggedToken<'t>> {
        if let Some(w) = self.ready.pop_front() {
            return Some(w);
        }
        self.refill();
        self.ready.pop_front()
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let buffered =
            self.ready.len() + (self.base + self.window.len()).saturating_sub(self.cursor);
        let (lo, hi) = self.src.size_hint();
        (lo + buffered, hi.and_then(|h| h.checked_add(buffered)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::corpus::Corpus;
    use crate::tag::Tag;

    fn tag(s: &'static str) -> Tag {
        Tag::new(s).unwrap()
    }

    /// A small Penn-tagged lexicon and rule set, standing in for the
    /// dictionaries this crate no longer ships.
    ///
    /// Everything here is written out rather than loaded, which is the point:
    /// these tests are about the engine, and the engine's inputs are now always
    /// the caller's. The rules cover the three shape conditions (`URL`, number,
    /// suffix) alongside an ordinary contextual one, because those are the
    /// conditions whose interaction with the initial state is worth pinning.
    fn fixture() -> (Lexicon, RuleSet) {
        let mut lexicon = Lexicon::new(tag("NN")).with_capitalized_default_tag(tag("NNP"));
        for (key, tags) in [
            (".", vec!["."]),
            ("a", vec!["DT"]),
            ("at", vec!["IN"]),
            ("book", vec!["NN", "VB"]),
            ("brown", vec!["JJ"]),
            ("dog", vec!["NN"]),
            ("flight", vec!["NN"]),
            ("fox", vec!["NN"]),
            ("has", vec!["VBZ"]),
            ("he", vec!["PRP"]),
            ("i", vec!["PRP"]),
            ("in", vec!["IN"]),
            ("jumps", vec!["VBZ"]),
            ("lazy", vec!["JJ"]),
            ("lives", vec!["VBZ"]),
            ("meeting", vec!["NN"]),
            ("more", vec!["JJR"]),
            ("over", vec!["IN"]),
            ("physics", vec!["NNS"]),
            ("president", vec!["NN"]),
            ("quick", vec!["JJ"]),
            ("quickly", vec!["RB"]),
            ("read", vec!["VB"]),
            ("runs", vec!["VBZ"]),
            ("she", vec!["PRP"]),
            ("the", vec!["DT"]),
            ("today", vec!["NN"]),
            ("visit", vec!["VB"]),
            ("would", vec!["MD"]),
            ("W.Va.", vec!["NNP"]),
        ] {
            lexicon
                .insert(key, tags.into_iter().map(tag).collect())
                .unwrap();
        }
        let rules: RuleSet = "NN VB PREV-TAG MD\n\
             NN CD CURRENT-WORD-IS-NUMBER YES\n\
             NN URL CURRENT-WORD-IS-URL YES\n\
             NNS URL CURRENT-WORD-IS-URL YES\n\
             NNP URL CURRENT-WORD-IS-URL YES\n\
             NNPS URL CURRENT-WORD-IS-URL YES\n\
             NN NNS CURRENT-WORD-ENDS-WITH s\n\
             NN VBG CURRENT-WORD-ENDS-WITH ing"
            .parse()
            .unwrap();
        (lexicon, rules)
    }

    /// A lexicon and rule set whose effect propagates far further than any one
    /// rule's reach, in **both** directions.
    ///
    /// Rule *k* rewrites a tag that rule *k-1* created, so the consequence of a
    /// single `seed` token travels `N` positions outward — driven entirely by
    /// one-token context reads. That makes `context_span` load-bearing rather
    /// than nominal: a streaming implementation that buffered less than the sum
    /// would give a different answer from a whole-document run, which is
    /// precisely what `streaming_matches_batch_under_a_wide_context` checks.
    fn ripple() -> (Lexicon, RuleSet) {
        use std::fmt::Write as _;
        const N: usize = 12;

        let mut lexicon = Lexicon::new(tag("A"));
        lexicon.insert("seed", vec![tag("S")]).unwrap();
        let mut text = String::new();
        for k in 0..N {
            let from = if k == 0 {
                "S".to_owned()
            } else {
                format!("T{k}")
            };
            writeln!(text, "A T{} PREV-TAG {from}", k + 1).unwrap();
            let from = if k == 0 {
                "S".to_owned()
            } else {
                format!("U{k}")
            };
            writeln!(text, "A U{} NEXT-TAG {from}", k + 1).unwrap();
        }
        let rules: RuleSet = text.parse().unwrap();
        assert_eq!(rules.context_span(), (N, N));
        (lexicon, rules)
    }

    fn tags_of(words: &[TaggedToken<'_>]) -> Vec<String> {
        words.iter().map(|w| w.tag().to_string()).collect()
    }

    /// Every tag below is read off the fixture, not off a previous run: `would`
    /// is `MD`, `a` is `DT` and `flight` is `NN` in the lexicon, `book` is `NN`
    /// there and becomes `VB` through `NN VB PREV-TAG MD`, and `I` has no entry
    /// of its own so the lowercase retry finds `i`.
    #[test]
    fn the_textbook_sentence() {
        let (lex, rules) = fixture();
        let t = BrillTagger::new(&lex, &rules);
        assert_eq!(
            tags_of(&t.tag(["I", "would", "book", "a", "flight"])),
            ["PRP", "MD", "VB", "DT", "NN"]
        );
        assert_eq!(lex.primary_tag("book"), Some(tag("NN")));
        assert_eq!(lex.primary_tag("i"), Some(tag("PRP")));
        assert!(!lex.contains("I"));
    }

    /// The URL heuristic is small enough not to steal abbreviations.
    ///
    /// The four `… URL CURRENT-WORD-IS-URL YES` rules rewrite `NN`, `NNS`, `NNP`
    /// and `NNPS`, so every token the heuristic wrongly accepts overrides a noun
    /// the lexicon stated. `W.Va.` is `NNP` in the fixture; `Ph.D.` is not in it
    /// at all and takes the capitalised default, which is `NNP` too — so the
    /// exposure is not limited to keys the lexicon happens to hold. A
    /// sentence-final URL, the shape whitespace tokenization produces most
    /// often, still has to reach `URL`.
    #[test]
    fn abbreviations_keep_their_tag_while_a_sentence_final_url_still_reaches_url() {
        let (lex, rules) = fixture();
        let t = BrillTagger::new(&lex, &rules);
        assert_eq!(lex.primary_tag("W.Va."), Some(tag("NNP")));
        assert_eq!(lex.primary_tag("Ph.D."), None);
        assert_eq!(lex.tag_of("Ph.D."), tag("NNP"));

        assert_eq!(
            tags_of(&t.tag("She has a Ph.D. in physics".split(' '))),
            ["PRP", "VBZ", "DT", "NNP", "IN", "NNS"]
        );
        assert_eq!(
            tags_of(&t.tag("He lives in W.Va.".split(' '))),
            ["PRP", "VBZ", "IN", "NNP"]
        );
        assert_eq!(
            tags_of(&t.tag("the president-U.S. meeting".split(' '))),
            ["DT", "NN", "VBG"]
        );

        // ...while both shapes of URL do become `URL`.
        assert_eq!(
            tags_of(&t.tag("Visit www.example.com. today".split(' '))),
            ["VB", "URL", "NN"]
        );
        assert_eq!(
            tags_of(&t.tag("Read more at www.example.com".split(' '))),
            ["VB", "JJR", "IN", "URL"]
        );
    }

    #[test]
    fn suffix_rules_fire_on_real_suffixes() {
        let (lex, rules) = fixture();
        let t = BrillTagger::new(&lex, &rules);
        // Tokens absent from the lexicon, so the initial state is the `NN`
        // default and only the suffix rules can move them.
        assert_eq!(
            tags_of(&t.tag(["zzcats", "zzsees", "zzstring", "zzsinging", "zzss"])),
            ["NNS", "NNS", "VBG", "VBG", "NNS"]
        );
    }

    #[test]
    fn rule_order_is_observable() {
        let mut lex = Lexicon::new(tag("NN"));
        lex.insert("the", vec![tag("DT")]).unwrap();
        let words = ["the", "a", "b"];

        let forward: RuleSet = "NN VB PREV-TAG DT\nNN JJ NEXT-TAG NN".parse().unwrap();
        assert_eq!(
            tags_of(&BrillTagger::new(&lex, &forward).tag(words)),
            ["DT", "VB", "NN"]
        );
        let reverse: RuleSet = "NN JJ NEXT-TAG NN\nNN VB PREV-TAG DT".parse().unwrap();
        assert_eq!(
            tags_of(&BrillTagger::new(&lex, &reverse).tag(words)),
            ["DT", "JJ", "NN"]
        );
    }

    /// A rule decides every site against the tagging that existed before it ran,
    /// so its own rewrites are invisible to its own later tests. With
    /// left-to-right in-place application the second token would become `B` as
    /// well; with simultaneous application it does not.
    #[test]
    fn a_rule_does_not_see_its_own_rewrites() {
        let lex = Lexicon::new(tag("A"));
        let rules: RuleSet = "A B PREV-TAG A".parse().unwrap();
        let t = BrillTagger::new(&lex, &rules);
        assert_eq!(tags_of(&t.tag(["x", "y", "z", "w"])), ["A", "B", "B", "B"]);

        let rules: RuleSet = "A B PREV-TAG B".parse().unwrap();
        let t = BrillTagger::new(&lex, &rules);
        assert_eq!(
            tags_of(&t.tag(["x", "y", "z", "w"])),
            ["A", "A", "A", "A"],
            "no B exists before the rule runs, so nothing can chain off one"
        );
    }

    /// A later rule *does* see an earlier rule's rewrites: that is what makes
    /// the sequence a sequence.
    #[test]
    fn a_later_rule_sees_an_earlier_rules_rewrites() {
        let lex = Lexicon::new(tag("A"));
        let rules: RuleSet = "A B CURRENT-WORD-IS x\nA C PREV-TAG B".parse().unwrap();
        let t = BrillTagger::new(&lex, &rules);
        assert_eq!(tags_of(&t.tag(["x", "y", "z"])), ["B", "C", "A"]);
    }

    /// Tokens are returned exactly as supplied, whatever script or length.
    #[test]
    fn every_character_class_tags_without_rewriting_or_panicking() {
        let (lex, rules) = fixture();
        let t = BrillTagger::new(&lex, &rules);
        let long = "z".repeat(10_000);
        let cases: &[(&str, &str)] = &[
            ("", "NN"),
            ("z", "NN"),
            ("Z", "NNP"),
            ("Zzzzz", "NNP"),
            ("café", "NN"),
            ("Café", "NNP"),
            ("Ålesundzz", "NNP"),
            ("Москвазз", "NNP"),
            ("Ελλάςζζ", "NNP"),
            ("ΟΔΟΣΣΣ", "NNP"),
            ("İstanbulzz", "NNP"),
            ("straßezz", "NN"),
            ("日本語", "NN"),
            ("😀", "NN"),
            ("𝐀bc", "NNP"),
            (".", "."),
            ("5", "CD"),
            ("3.14", "CD"),
            ("www.example.com", "URL"),
            (&long, "NN"),
            ("a b", "NN"),
            ("\u{feff}", "NN"),
        ];
        for (token, want) in cases {
            let got = t.tag([*token]);
            assert_eq!(got[0].tag().as_str(), *want, "{token:?}");
            assert_eq!(got[0].token(), *token, "token was rewritten");
        }
    }

    #[test]
    fn empty_and_single_token_input() {
        let (lex, rules) = fixture();
        let t = BrillTagger::new(&lex, &rules);
        assert!(t.tag(Vec::<&str>::new()).is_empty());
        assert!(t.tag_stream(Vec::<&str>::new()).next().is_none());
        assert_eq!(tags_of(&t.tag(["a"])), ["DT"]);
    }

    /// The four ways of producing tags must agree on every length, including the
    /// lengths around the streaming block boundary.
    #[test]
    fn all_apis_agree_on_every_length() {
        let (lex, rules) = fixture();
        let t = BrillTagger::new(&lex, &rules);
        let words: Vec<&str> = [
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
        ]
        .into_iter()
        .cycle()
        .take(2100)
        .collect();

        for n in (0..=64).chain([1023, 1024, 1025, 2047, 2048, 2049, 2100]) {
            let slice = &words[..n];
            let batch = t.tag(slice.iter().copied());
            let streamed: Vec<TaggedToken<'_>> = t.tag_stream(slice.iter().copied()).collect();
            assert_eq!(streamed, batch, "tag_stream diverges at length {n}");

            let mut buf = Vec::new();
            t.tag_into(slice.iter().copied(), &mut buf);
            assert_eq!(buf, batch, "tag_into diverges at length {n}");

            let mut annotated = t.annotate(slice.iter().copied());
            t.transform(&mut annotated);
            assert_eq!(
                annotated, batch,
                "annotate+transform diverges at length {n}"
            );
        }
    }

    /// The same equivalence under a rule set whose reach is 12 tokens on each
    /// side and whose effects genuinely propagate that far, so the streaming
    /// margin is doing work rather than being generous.
    #[test]
    fn streaming_matches_batch_under_a_wide_context() {
        let (lex, rules) = ripple();
        let t = BrillTagger::new(&lex, &rules);
        let (left, right) = rules.context_span();
        assert!(
            left >= 12 && right >= 12,
            "context is wide enough to matter"
        );

        // Seeds at a period coprime with the 1024-token block, so a ripple falls
        // across a block boundary at some point in the sweep.
        let period = 37;
        let words: Vec<&str> = (0..3000)
            .map(|i| if i % period == 0 { "seed" } else { "x" })
            .collect();

        for n in [0, 1, 7, 100, 1023, 1024, 1030, 2048, 2100, 3000] {
            let slice = &words[..n];
            let batch = t.tag(slice.iter().copied());
            let streamed: Vec<TaggedToken<'_>> = t.tag_stream(slice.iter().copied()).collect();
            assert_eq!(streamed, batch, "length {n}");
        }

        // ...and the ripple really does reach 12 positions out, or the test
        // above would pass on a rule set that never used its context.
        let short: Vec<&str> = std::iter::once("seed")
            .chain(std::iter::repeat_n("x", 20))
            .collect();
        let got = tags_of(&t.tag(short.iter().copied()));
        assert_eq!(got[0], "S");
        assert_eq!(got[12], "T12", "the chain travelled twelve tokens");
        assert_eq!(got[13], "A", "and no further");
    }

    #[test]
    fn streaming_holds_a_bounded_window() {
        let (lex, rules) = fixture();
        let t = BrillTagger::new(&lex, &rules);
        let n = 50_000;
        let mut stream = t.tag_stream(std::iter::repeat_n("dog", n));
        let mut count = 0;
        while let Some(w) = stream.next() {
            assert_eq!(w.tag().as_str(), "NN");
            count += 1;
            assert!(
                stream.window.len() <= STREAM_BLOCK + 8,
                "window grew to {}",
                stream.window.len()
            );
        }
        assert_eq!(count, n);
    }

    #[test]
    fn tag_into_appends_and_leaves_the_prefix_alone() {
        let (lex, rules) = fixture();
        let t = BrillTagger::new(&lex, &rules);
        let mut buf = t.tag(["would", "book"]);
        let before = buf.clone();
        t.tag_into(["a", "flight"], &mut buf);
        assert_eq!(&buf[..2], &before[..]);
        assert_eq!(tags_of(&buf[2..]), ["DT", "NN"]);
    }

    #[test]
    fn transform_with_clears_the_scratch_buffer() {
        let (lex, rules) = fixture();
        let t = BrillTagger::new(&lex, &rules);
        let mut sites = vec![9999, 12345];
        let mut words = t.annotate(["would", "book"]);
        t.transform_with(&mut words, &mut sites);
        assert_eq!(tags_of(&words), ["MD", "VB"]);
    }

    #[test]
    fn evaluation_reports_counts_and_never_nan() {
        let mut lex = Lexicon::new(tag("NN"));
        lex.insert("to", vec![tag("TO")]).unwrap();
        lex.insert("book", vec![tag("NN")]).unwrap();
        let corpus = Corpus::parse_brown("to_TO book_VB").unwrap();

        let none = RuleSet::new();
        let ev = BrillTagger::new(&lex, &none).evaluate(&corpus);
        assert_eq!(ev.tokens, 2);
        assert_eq!(ev.correct_before_rules, 1);
        assert_eq!(ev.correct_after_rules, 1);
        assert_eq!(ev.accuracy(), Some(0.5));

        let fix: RuleSet = "NN VB PREV-TAG TO".parse().unwrap();
        let ev = BrillTagger::new(&lex, &fix).evaluate(&corpus);
        assert_eq!(ev.accuracy_before_rules(), Some(0.5));
        assert_eq!(ev.accuracy(), Some(1.0));

        let empty = Corpus::new();
        let ev = BrillTagger::new(&lex, &fix).evaluate(&empty);
        assert_eq!(ev.tokens, 0);
        assert_eq!(ev.accuracy(), None, "0/0 is None, never NaN");
        assert_eq!(ev.accuracy_before_rules(), None);
    }

    #[test]
    fn taggers_and_lexicons_cross_threads() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<Lexicon>();
        assert_send_sync::<RuleSet>();
        assert_send_sync::<BrillTagger<'_>>();
        assert_send_sync::<TaggedToken<'_>>();
    }

    #[cfg(feature = "parallel")]
    #[test]
    fn par_tag_batch_matches_the_sequential_loop() {
        let (lex, rules) = fixture();
        let t = BrillTagger::new(&lex, &rules);
        let documents: Vec<Vec<&str>> = vec![
            vec![],
            vec!["a"],
            vec!["I", "would", "book", "a", "flight"],
            vec![
                "",
                "z",
                "Z",
                "café",
                "Ålesund",
                "Москва",
                "日本語",
                "😀",
                ".",
                "5",
                "3.14",
                "www.example.com",
            ],
            std::iter::repeat_n("dog", 3000).collect(),
        ];
        let sequential: Vec<_> = documents.iter().map(|d| t.tag(d.iter().copied())).collect();
        assert_eq!(t.par_tag_batch(&documents), sequential);
    }
}
