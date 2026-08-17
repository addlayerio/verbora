//! [`SentenceAnalyzer`] — a line-for-line port of
//! The reference `sentence_analyzer`.

use std::fmt;

use crate::sen_type::SenType;
use crate::tagged::{NoPunct, Punct, TaggedSentence, TaggedWord, no_punct};

/// The reference `pos.match('IN')` / `pos.match('NN')`, without a regex engine.
///
/// `String.prototype.match` compiles a *string* argument into a `RegExp`, so
/// these two calls are plain substring containment — `'VBIN'` matches `'IN'`
/// and `'NNS'`, `'NNP'`, `'NNPS'` all match `'NN'`. A port that reads the
/// source as `pos === 'IN'` mis-tags every plural and proper noun.
///
/// Both needles are ASCII, and an ASCII byte can never appear inside a
/// multi-byte UTF-8 sequence, so scanning the UTF-8 bytes gives exactly the
/// same answer as the reference's UTF-16 search — no decoding, no allocation. POS
/// tags are two to four bytes, which is far below the length where a general
/// substring searcher's setup cost pays for itself, hence the open-coded
/// two-byte window.
#[inline]
fn contains_pair(haystack: &str, needle: [u8; 2]) -> bool {
    let bytes = haystack.as_bytes();
    if bytes.len() < 2 {
        return false;
    }
    bytes
        .windows(2)
        .any(|w| w[0] == needle[0] && w[1] == needle[1])
}

/// Why [`SentenceAnalyzer::type_of`] could not classify the sentence.
///
/// Both variants correspond to a reference `TypeError` — `type()` reads a
/// property off a value the reference never checks for existence. They are
/// distinct here because the two failures happen at different points and a
/// caller may want to tell them apart; the reference reports the same message for
/// both, which [`Self::message_text`] returns.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum TypeError {
    /// `punct()` was empty *and* `tags` was empty, so the popped last element
    /// was `undefined`.
    MissingLastElement,
    /// The popped last element existed but was not punctuation, and `tags` was
    /// empty when `tags[0].pos` was read — either because the sentence had a
    /// single tag that the pop consumed, or because it was empty to begin with
    /// and `punct()` supplied the last element.
    MissingFirstTag,
}

impl TypeError {
    /// The exact message the reference throws for this input.
    ///
    /// The reference engine reports the same text for both variants, because both are a property
    /// read on `undefined` with the same property name.
    #[must_use]
    pub const fn message_text(self) -> &'static str {
        "Cannot read properties of undefined (reading 'pos')"
    }
}

impl fmt::Display for TypeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingLastElement => f.write_str(
                "cannot determine sentence type: punct() was empty and there are no tags to pop",
            ),
            Self::MissingFirstTag => f.write_str(
                "cannot determine sentence type: no tags remain to inspect for a wh-word",
            ),
        }
    }
}

impl std::error::Error for TypeError {}

/// Classifies a POS-tagged sentence and splits it into subject and predicate.
///
/// Ports the reference `sentence_analyzer`. The input is tagged text
/// the caller supplies — nothing in the reference produces the `{ tags, punct }`
/// shape, so this analyzer sits downstream of whatever tagger you use.
///
/// ```
/// use verbora_analyzers::{SenType, SentenceAnalyzer, TaggedWord as W};
///
/// let mut a = SentenceAnalyzer::new(vec![
///     W::new("The", "DT"),
///     W::new("angry", "JJ"),
///     W::new("bear", "NN"),
///     W::new("chased", "VB"),
///     W::new("the", "DT"),
///     W::new("squirrel", "NN"),
///     W::new(".", "."),
/// ]);
/// a.part();
/// assert_eq!(a.subject_to_string().trim(), "The angry bear");
/// // The full stop is a tag like any other, so it lands in the predicate…
/// assert_eq!(a.predicate_to_string().trim(), "chased the squirrel .");
/// assert_eq!(a.type_of(), Ok(Some(SenType::Declarative)));
/// // …until `type_of` pops it off to classify the sentence.
/// assert_eq!(a.to_string(), "The angry bear chased the squirrel");
/// ```
///
/// # Every method mutates
///
/// This is not a pure classifier, and pretending otherwise would break parity:
///
/// * [`Self::preposition_phrases`] sets `pp` on tags.
/// * [`Self::part`] sets `spos`, and appends an implicit-'You' subject to a
///   verb-initial sentence — **again** on a second call, so it is not
///   idempotent.
/// * [`Self::type_of`] *pops the last tag* whenever `punct()` is empty.
///
/// # The callback-driven constructor
///
/// The reference's constructor takes a callback and invokes it synchronously:
/// `new SentenceAnalyzer(pos, cb)`. Nothing about the work is asynchronous, so
/// the Rust constructors just return the analyzer; [`Self::part_with`] and
/// [`Self::type_with`] are kept for code being ported callback-first. Passing a
/// non-function callback throws in the reference, which Rust's type system rules
/// out.
pub struct SentenceAnalyzer<'a, P = NoPunct<'a>> {
    /// The reference's `posObj`, held by reference there and by value here.
    pub pos_obj: TaggedSentence<'a, P>,
    /// The classification, `None` until [`Self::type_of`] sets it.
    ///
    /// It can *stay* `None` after a successful call: the `switch` in `type()`
    /// has no default arm.
    pub sen_type: Option<SenType>,
}

impl<'a> SentenceAnalyzer<'a, NoPunct<'a>> {
    /// Builds an analyzer whose `punct()` always reports no punctuation.
    ///
    /// This matches `punct: function () { return [] }`, the accessor used by
    /// every example in the reference's own spec — and it is the case in which
    /// [`Self::type_of`] pops the sentence's last tag.
    #[must_use]
    pub fn new(tags: Vec<TaggedWord<'a>>) -> Self {
        Self {
            pos_obj: TaggedSentence::new(tags, no_punct as NoPunct<'a>),
            sen_type: None,
        }
    }
}

impl<'a, P> SentenceAnalyzer<'a, P> {
    /// Builds an analyzer with an explicit `punct()` accessor.
    ///
    /// ```
    /// use verbora_analyzers::{Punct, SenType, SentenceAnalyzer, TaggedWord as W};
    ///
    /// // The reference `function () { return [{ token: '?', pos: '.' }] }`
    /// // returns a fresh array per call, so the closure rebuilds it per call.
    /// let mut a = SentenceAnalyzer::with_punct(
    ///     vec![W::new("Do", "VB"), W::new("I", "PRP"), W::new("care", "VB")],
    ///     || Punct::List(vec![W::new("?", ".")]),
    /// );
    /// assert_eq!(a.type_of(), Ok(Some(SenType::Interrogative)));
    /// // Punctuation came from `punct()`, so no tag was consumed.
    /// assert_eq!(a.pos_obj.tags.len(), 3);
    /// ```
    pub fn with_punct(tags: Vec<TaggedWord<'a>>, punct: P) -> Self {
        Self {
            pos_obj: TaggedSentence::new(tags, punct),
            sen_type: None,
        }
    }

    // -- structure ---------------------------------------------------------

    /// Flags the tags that lie inside a prepositional phrase.
    ///
    /// Scans left to right: a tag whose `pos` *contains* `IN` opens a phrase,
    /// one whose `pos` contains `NN` closes it. Two details that a paraphrase
    /// of the source loses:
    ///
    /// * the closing `NN` tag is itself flagged, because `pp` is written before
    ///   the flag is cleared;
    /// * `pp` is only ever set, never removed, so repeated calls only add.
    ///
    /// ```
    /// use verbora_analyzers::{SentenceAnalyzer, TaggedWord as W};
    ///
    /// let mut a = SentenceAnalyzer::new(vec![
    ///     W::new("cat", "NN"),
    ///     W::new("in", "IN"),
    ///     W::new("the", "DT"),
    ///     W::new("hats", "NNS"),  // NNS contains NN: closes the phrase…
    ///     W::new("today", "RB"),
    /// ]);
    /// a.preposition_phrases();
    /// let flags: Vec<bool> = a.pos_obj.tags.iter().map(|t| t.is_pp()).collect();
    /// assert_eq!(flags, [false, true, true, true, false]); // …and is flagged
    /// ```
    pub fn preposition_phrases(&mut self) {
        let mut inside = false;
        for tag in &mut self.pos_obj.tags {
            if contains_pair(tag.pos(), *b"IN") {
                inside = true;
            }
            if inside {
                tag.set_pp(true);
            }
            // Deliberately after the write: the noun that ends the phrase is
            // flagged too.
            if contains_pair(tag.pos(), *b"NN") {
                inside = false;
            }
        }
    }

    /// Splits the sentence into subject and predicate, annotating `spos`.
    ///
    /// Runs [`Self::preposition_phrases`] first, then walks the tags: everything
    /// before the first verb is subject (`SP`), everything from the verb on is
    /// predicate (`PP`). Tags already flagged `pp` are skipped — they keep *no*
    /// `spos` key at all.
    ///
    /// Three faithfully-reproduced oddities:
    ///
    /// * only the exact tag `VB` counts as a verb; `VBD`, `VBG`, `VBN`, `VBP`
    ///   and `VBZ` do not, because the comparison is `===`.
    /// * a `VB` preceded by an existential `EX` does **not** open the predicate,
    ///   so "There is a house" is subject all the way through.
    /// * when nothing landed in the subject — a verb-initial sentence, or no
    ///   tags at all — an implicit `You` subject is appended. Calling `part`
    ///   twice appends a *second* one.
    pub fn part(&mut self) {
        self.preposition_phrases();

        let mut verb_found = false;
        let mut subject_empty = true;
        // The reference reads `tags[i - 1].pos`; carrying it forward avoids
        // indexing and is equivalent because `pos` is never written here.
        let mut prev_is_existential = false;

        for (i, tag) in self.pos_obj.tags.iter_mut().enumerate() {
            if tag.pos() == "VB" && (i == 0 || !prev_is_existential) {
                verb_found = true;
            }
            prev_is_existential = tag.pos() == "EX";

            if verb_found {
                if !tag.is_pp() {
                    tag.set_spos("PP");
                }
            } else {
                if !tag.is_pp() {
                    tag.set_spos("SP");
                }
                subject_empty = false;
            }
        }

        if subject_empty {
            self.pos_obj.tags.push(TaggedWord::implicit_you());
        }
    }

    /// [`Self::part`], then a callback — the shape of the reference's `part(cb)`.
    ///
    /// The callback sees every mutation, because the reference invokes it as
    /// the last statement of the method.
    pub fn part_with(&mut self, callback: impl FnOnce(&mut Self)) {
        self.part();
        callback(self);
    }

    // -- rendering ---------------------------------------------------------

    /// The tokens, one per tag, in order.
    pub fn tokens(&self) -> impl ExactSizeIterator<Item = &str> + '_ {
        self.pos_obj.tags.iter().map(TaggedWord::token)
    }

    /// One slot per tag: the token when the tag is part of the subject, and the
    /// **empty string** when it is not.
    ///
    /// This is the lazy primitive behind [`Self::subject_to_string`], and the
    /// empty slots are not an implementation detail: the reference maps
    /// non-subject tags to `null` and `Array#join` renders `null` as `''`, so
    /// the joined string is full of stray spaces. Use
    /// [`Self::subject_tokens`] if you want the tokens without them.
    ///
    /// `spos === 'S'` is accepted alongside `'SP'`; nothing in the reference ever
    /// writes `'S'`, but the comparison is there and a caller can pre-set it.
    pub fn subject_slots(&self) -> impl ExactSizeIterator<Item = &str> + '_ {
        self.pos_obj.tags.iter().map(|t| slot(t, "SP", "S"))
    }

    /// One slot per tag: the token when the tag is part of the predicate, the
    /// empty string when it is not. See [`Self::subject_slots`].
    pub fn predicate_slots(&self) -> impl ExactSizeIterator<Item = &str> + '_ {
        self.pos_obj.tags.iter().map(|t| slot(t, "PP", "P"))
    }

    /// Just the subject's tokens, with the empty slots dropped.
    ///
    /// **Not** what the reference returns — see [`Self::subject_to_string`] — but
    /// what most callers of that method actually wanted.
    pub fn subject_tokens(&self) -> impl Iterator<Item = &str> + '_ {
        self.subject_slots().filter(|s| !s.is_empty())
    }

    /// Just the predicate's tokens, with the empty slots dropped.
    pub fn predicate_tokens(&self) -> impl Iterator<Item = &str> + '_ {
        self.predicate_slots().filter(|s| !s.is_empty())
    }

    /// The subject, joined with single spaces — including the empty slots.
    ///
    /// ```
    /// use verbora_analyzers::{SentenceAnalyzer, TaggedWord as W};
    ///
    /// let mut a = SentenceAnalyzer::new(vec![
    ///     W::new("The", "DT"), W::new("bear", "NN"), W::new("ran", "VB"),
    /// ]);
    /// a.part();
    /// // One trailing space: the predicate tag still occupies a slot.
    /// assert_eq!(a.subject_to_string(), "The bear ");
    /// assert_eq!(a.subject_to_string().trim(), "The bear");
    /// ```
    #[must_use]
    pub fn subject_to_string(&self) -> String {
        join_with_spaces(self.subject_slots())
    }

    /// The predicate, joined with single spaces — including the empty slots.
    #[must_use]
    pub fn predicate_to_string(&self) -> String {
        join_with_spaces(self.predicate_slots())
    }

    /// Whether the analyzer added an implicit `You` subject.
    ///
    /// Note the tense: this reports the *presence of an added tag*, so it is
    /// `false` before [`Self::part`] runs and stays `true` afterwards even once
    /// [`Self::type_of`] has popped the `You` back off.
    #[must_use]
    pub fn implicit_you(&self) -> bool {
        self.pos_obj.tags.iter().any(TaggedWord::is_added)
    }
}

/// The classification step is the only one that calls `punct()`, so it is the
/// only one that constrains `P`. Annotating and rendering a sentence works
/// whatever the accessor is — including not having one at all.
impl<'a, P: FnMut() -> Punct<'a>> SentenceAnalyzer<'a, P> {
    /// Classifies the sentence, and **consumes its last tag** when `punct()` is
    /// empty.
    ///
    /// The algorithm, in the reference's own order:
    ///
    /// 1. Note whether the sentence has an implicit 'You' — *before* anything is
    ///    popped, so a `You` that this call removes still counts.
    /// 2. Call `punct()`. If what comes back has length 0 (an empty array **or**
    ///    the empty string), pop the last tag off the sentence and use that;
    ///    otherwise pop the last mark off the returned list, leaving the
    ///    sentence untouched.
    /// 3. If that element's `pos` is not `'.'`: an implicit 'You' means
    ///    `COMMAND`; else a leading `WDT`/`WP`/`WP$`/`WRB` means
    ///    `INTERROGATIVE`; else a trailing `PRP` — "…, should we" — also means
    ///    `INTERROGATIVE`; else `UNKNOWN`.
    /// 4. If it is `'.'`, switch on the token: `?` → `INTERROGATIVE`, `!` →
    ///    `COMMAND` or `EXCLAMATORY`, `.` → `COMMAND` or `DECLARATIVE`.
    ///
    /// # The missing default arm
    ///
    /// Step 4 has no fallback. A tag with `pos: '.'` whose token is anything
    /// else — a semicolon, say — leaves [`Self::sen_type`] **exactly as it
    /// was**, which is `None` on a fresh analyzer and the previous answer on a
    /// re-used one. Returning `SenType::Unknown` there would be a plausible
    /// reading of the source and a parity bug.
    ///
    /// ```
    /// use verbora_analyzers::{Punct, SentenceAnalyzer, TaggedWord as W};
    ///
    /// let mut a = SentenceAnalyzer::with_punct(
    ///     vec![W::new("Do", "VB"), W::new("I", "PRP")],
    ///     || Punct::List(vec![W::new(";", ".")]),
    /// );
    /// assert_eq!(a.type_of(), Ok(None));
    /// assert_eq!(a.sen_type, None);
    /// ```
    ///
    /// # Errors
    ///
    /// Returns [`TypeError`] where the reference throws a `TypeError`: when the
    /// pop finds no tag at all, and when the wh-word test runs with no tags
    /// left. Both are reachable — a one-tag sentence with an empty `punct()` is
    /// enough.
    ///
    /// Three further the reference throws have no Rust counterpart because
    /// [`Punct`] cannot express them: `punct()` returning `null` or `undefined`
    /// (`Cannot read properties of … (reading 'length')`), and `punct()`
    /// returning a *non-empty* string (`lastElement.pop is not a function`).
    pub fn type_of(&mut self) -> Result<Option<SenType>, TypeError> {
        // Computed first: step 1 is load-bearing precisely because step 2 can
        // delete the tag it looked at.
        let implicit_you = self.implicit_you();

        let last = match self.pos_obj.punct() {
            Punct::List(mut marks) if !marks.is_empty() => {
                marks.pop().expect("the list was just checked as non-empty")
            }
            // Empty array and empty string alike fall through to the sentence.
            _ => self
                .pos_obj
                .tags
                .pop()
                .ok_or(TypeError::MissingLastElement)?,
        };

        if last.pos() == "." {
            match last.token() {
                "?" => self.sen_type = Some(SenType::Interrogative),
                "!" => {
                    self.sen_type = Some(if implicit_you {
                        SenType::Command
                    } else {
                        SenType::Exclamatory
                    });
                }
                "." => {
                    self.sen_type = Some(if implicit_you {
                        SenType::Command
                    } else {
                        SenType::Declarative
                    });
                }
                // No default arm — see the note above.
                _ => {}
            }
        } else {
            let classified = if implicit_you {
                SenType::Command
            } else {
                // `tags[0]` is read *after* the pop, so this is the first tag of
                // what is left.
                let first = self
                    .pos_obj
                    .tags
                    .first()
                    .ok_or(TypeError::MissingFirstTag)?;
                if matches!(first.pos(), "WDT" | "WP" | "WP$" | "WRB") || last.pos() == "PRP" {
                    SenType::Interrogative
                } else {
                    SenType::Unknown
                }
            };
            self.sen_type = Some(classified);
        }

        Ok(self.sen_type)
    }

    /// [`Self::type_of`], then a callback — the shape of the reference's
    /// `type(cb)`.
    ///
    /// The reference conflates the two: `type(cbf)` returns `undefined` when
    /// `cbf` is a function and the classification otherwise, and its `.d.ts`
    /// declares `string` for both. Splitting them into two methods removes the
    /// ambiguity without changing any observable value.
    ///
    /// # Errors
    ///
    /// As [`Self::type_of`]. The callback is not invoked when classification
    /// fails, matching the reference, where the throw happens first.
    pub fn type_with(&mut self, callback: impl FnOnce(&mut Self)) -> Result<(), TypeError> {
        self.type_of()?;
        callback(self);
        Ok(())
    }
}

/// One `map` slot of `subjectToString`/`predicateToString`.
#[inline]
fn slot<'t>(tag: &'t TaggedWord<'_>, long: &str, short: &str) -> &'t str {
    match tag.spos() {
        Some(s) if s == long || s == short => tag.token(),
        _ => "",
    }
}

/// `Array#join(' ')`, including the empty slots the reference's `null`s become.
fn join_with_spaces<'s>(slots: impl ExactSizeIterator<Item = &'s str>) -> String {
    let separators = slots.len().saturating_sub(1);
    let mut out = String::with_capacity(separators);
    for (i, s) in slots.enumerate() {
        if i > 0 {
            out.push(' ');
        }
        out.push_str(s);
    }
    out
}

/// The reference's `toString()`: every token, joined with single spaces.
///
/// Includes an appended implicit 'You' and excludes anything [`SentenceAnalyzer::type_of`]
/// has popped, because it reads the live tag list.
impl<P> fmt::Display for SentenceAnalyzer<'_, P> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (i, tag) in self.pos_obj.tags.iter().enumerate() {
            if i > 0 {
                f.write_str(" ")?;
            }
            f.write_str(tag.token())?;
        }
        Ok(())
    }
}

impl<P> fmt::Debug for SentenceAnalyzer<'_, P> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SentenceAnalyzer")
            .field("pos_obj", &self.pos_obj)
            .field("sen_type", &self.sen_type)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn w<'a>(token: &'a str, pos: &'a str) -> TaggedWord<'a> {
        TaggedWord::new(token, pos)
    }

    #[test]
    fn contains_pair_is_substring_not_equality() {
        assert!(contains_pair("IN", *b"IN"));
        assert!(contains_pair("VBIN", *b"IN"));
        assert!(contains_pair("XINY", *b"IN"));
        assert!(!contains_pair("in", *b"IN"));
        assert!(!contains_pair("I", *b"IN"));
        assert!(!contains_pair("", *b"IN"));
        for p in ["NN", "NNS", "NNP", "NNPS", "XNNY"] {
            assert!(contains_pair(p, *b"NN"), "{p}");
        }
        // A multi-byte character can never produce a false positive: no ASCII
        // byte occurs inside a UTF-8 continuation sequence.
        assert!(!contains_pair("日本語", *b"IN"));
        assert!(contains_pair("😀IN", *b"IN"));
    }

    #[test]
    fn bear_sentence_matches_the_reference_spec() {
        let mut a = SentenceAnalyzer::new(vec![
            w("The", "DT"),
            w("angry", "JJ"),
            w("bear", "NN"),
            w("chased", "VB"),
            w("the", "DT"),
            w("frightened", "JJ"),
            w("little", "JJ"),
            w("squirrel", "NN"),
        ]);
        a.part();
        assert_eq!(a.pos_obj.tags[1].spos(), Some("SP"));
        assert_eq!(a.pos_obj.tags[7].spos(), Some("PP"));
        assert_eq!(a.subject_to_string().trim(), "The angry bear");
        assert_eq!(
            a.predicate_to_string().trim(),
            "chased the frightened little squirrel"
        );
        assert_eq!(
            a.to_string(),
            "The angry bear chased the frightened little squirrel"
        );
        assert!(!a.implicit_you());
    }

    #[test]
    fn verb_initial_sentence_gains_an_implicit_you() {
        let mut a = SentenceAnalyzer::new(vec![w("Vote", "VB"), w("for", "IN"), w("me", "PRP")]);
        a.part();
        assert_eq!(a.pos_obj.tags[0].spos(), Some("PP"));
        assert!(a.pos_obj.tags[1].is_pp());
        assert!(a.pos_obj.tags[2].is_pp());
        let you = a.pos_obj.tags.last().expect("a tag was appended");
        assert_eq!(you.token(), "You");
        assert_eq!(you.pos(), "PRP");
        assert!(you.is_added());
        assert!(a.implicit_you());
        assert_eq!(a.subject_to_string(), "   You");
        assert_eq!(a.predicate_to_string(), "Vote   ");
    }

    #[test]
    fn part_is_not_idempotent() {
        let mut a = SentenceAnalyzer::new(vec![w("Vote", "VB"), w("me", "PRP")]);
        a.part();
        a.part();
        let tokens: Vec<&str> = a.tokens().collect();
        assert_eq!(tokens, ["Vote", "me", "You", "You"]);
        // The first 'You' is now in the predicate: it followed the verb.
        assert_eq!(a.pos_obj.tags[2].spos(), Some("PP"));
        assert_eq!(a.pos_obj.tags[3].spos(), Some("SP"));
    }

    #[test]
    fn existential_there_keeps_the_verb_in_the_subject() {
        let mut a = SentenceAnalyzer::new(vec![
            w("There", "EX"),
            w("is", "VB"),
            w("a", "DT"),
            w("house", "NN"),
            w("in", "IN"),
            w("the", "DT"),
            w("valley", "DT"),
        ]);
        a.part();
        assert_eq!(a.subject_to_string().trim(), "There is a house");
        assert_eq!(a.predicate_to_string().trim(), "");
        assert_eq!(a.to_string(), "There is a house in the valley");
        assert!(!a.implicit_you());
    }

    #[test]
    fn empty_sentence_still_gains_a_you() {
        let mut a = SentenceAnalyzer::new(Vec::new());
        a.part();
        assert_eq!(a.tokens().collect::<Vec<_>>(), ["You"]);
    }

    #[test]
    fn type_pops_the_last_tag_when_punct_is_empty() {
        let mut a = SentenceAnalyzer::new(vec![w("I", "PRP"), w("am", "VB"), w("unsure", "JJ")]);
        assert_eq!(a.type_of(), Ok(Some(SenType::Unknown)));
        assert_eq!(a.tokens().collect::<Vec<_>>(), ["I", "am"]);
    }

    #[test]
    fn type_errors_where_the_reference_throws() {
        let mut empty = SentenceAnalyzer::new(Vec::new());
        assert_eq!(empty.type_of(), Err(TypeError::MissingLastElement));

        let mut single = SentenceAnalyzer::new(vec![w("x", "NN")]);
        assert_eq!(single.type_of(), Err(TypeError::MissingFirstTag));
        // The pop happened before the failure, exactly as in the reference.
        assert!(single.pos_obj.tags.is_empty());
    }

    #[test]
    fn trailing_pronoun_reads_as_a_tag_question() {
        let mut a = SentenceAnalyzer::with_punct(vec![w("Should", "MD"), w("we", "PRP")], || {
            Punct::EmptyString
        });
        assert_eq!(a.type_of(), Ok(Some(SenType::Interrogative)));
    }

    #[test]
    fn wh_word_first_reads_as_a_question() {
        for pos in ["WDT", "WP", "WP$", "WRB"] {
            let mut a = SentenceAnalyzer::new(vec![w("Who", pos), w("voted", "VB")]);
            assert_eq!(a.type_of(), Ok(Some(SenType::Interrogative)), "{pos}");
        }
        // Not first: no match.
        let mut a = SentenceAnalyzer::new(vec![w("a", "NN"), w("who", "WP"), w("z", "JJ")]);
        assert_eq!(a.type_of(), Ok(Some(SenType::Unknown)));
    }

    #[test]
    fn sen_type_survives_an_unmatched_switch() {
        let mut a =
            SentenceAnalyzer::with_punct(vec![w("a", "NN"), w("b", "NN"), w("c", "NN")], || {
                Punct::List(vec![w(";", ".")])
            });
        assert_eq!(a.type_of(), Ok(None));
        // Swap in an empty punct so the next call classifies, then swap back.
        let mut b = SentenceAnalyzer::new(vec![w("a", "NN"), w("b", "NN"), w("c", "NN")]);
        assert_eq!(b.type_of(), Ok(Some(SenType::Unknown)));
        let mut c = SentenceAnalyzer::with_punct(vec![w("a", "NN"), w("b", "NN")], || {
            Punct::List(vec![w(";", ".")])
        });
        c.sen_type = Some(SenType::Declarative);
        assert_eq!(c.type_of(), Ok(Some(SenType::Declarative)));
    }

    #[test]
    fn commands_win_over_punctuation() {
        for (token, expected) in [
            (".", SenType::Command),
            ("!", SenType::Command),
            ("?", SenType::Interrogative),
        ] {
            let mut a = SentenceAnalyzer::with_punct(
                vec![w("Vote", "VB"), w("for", "IN"), w("me", "PRP")],
                move || Punct::List(vec![TaggedWord::new(token.to_owned(), ".")]),
            );
            a.part();
            assert_eq!(a.type_of(), Ok(Some(expected)), "{token}");
        }
    }

    #[test]
    fn callback_forms_see_the_mutations() {
        let mut a = SentenceAnalyzer::new(vec![w("Vote", "VB"), w("me", "PRP")]);
        let mut seen = String::new();
        a.part_with(|x| seen = x.to_string());
        assert_eq!(seen, "Vote me You");

        let mut ty = None;
        a.type_with(|x| ty = x.sen_type).expect("classifiable");
        assert_eq!(ty, Some(SenType::Command));
    }

    #[test]
    fn tokens_survive_exotic_text() {
        // Tokens are only ever concatenated, so the interesting property is
        // that nothing is re-indexed: astral characters, combining marks and
        // whitespace exotica must come through byte-for-byte.
        let exotic = [
            "",
            " ",
            "café",
            "Москва",
            "日本語",
            "😀",
            "a😀b",
            "\u{feff}",
        ];
        let tags: Vec<TaggedWord<'_>> = exotic.iter().map(|t| w(t, "NN")).collect();
        let mut a = SentenceAnalyzer::new(tags);
        a.part();
        assert_eq!(a.to_string(), exotic.join(" "));
        assert_eq!(a.subject_to_string(), exotic.join(" "));
        assert_eq!(a.predicate_to_string(), " ".repeat(exotic.len() - 1));
    }

    #[test]
    fn very_long_sentences_are_linear_and_correct() {
        let n = 20_000;
        let mut tags: Vec<TaggedWord<'static>> = Vec::with_capacity(n);
        for i in 0..n {
            tags.push(TaggedWord::new("w", if i == 1 { "VB" } else { "DT" }));
        }
        let mut a = SentenceAnalyzer::new(tags);
        a.part();
        assert_eq!(a.pos_obj.tags.len(), n);
        assert_eq!(a.pos_obj.tags[0].spos(), Some("SP"));
        assert_eq!(a.pos_obj.tags[n - 1].spos(), Some("PP"));
        assert_eq!(a.subject_to_string().len(), 1 + (n - 1));
    }
}
