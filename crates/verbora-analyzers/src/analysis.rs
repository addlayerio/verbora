use std::fmt;
use std::ops::Range;

use crate::sentence_type::SentenceType;
use crate::tag::TagClass;
use crate::terminator::Terminator;
use crate::word::TaggedWord;

/// The part of the sentence a word belongs to.
///
/// Every word of the input gets exactly one role, so the roles partition the
/// sentence: there is no "unassigned" state and no `Option`. The partition is
/// three-way over the sentence *body* plus the terminator, rather than the
/// two-way subject/predicate split a clause diagram would draw, because a
/// prepositional phrase is analysed as its own constituent and is reported by
/// [`SentenceAnalysis::prepositional_phrases`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Role {
    /// Part of the subject: before the first verb, and outside every
    /// prepositional phrase.
    Subject,
    /// Part of the predicate: at or after the first verb, and outside every
    /// prepositional phrase. In an imperative clause every body word outside a
    /// phrase is predicate, because the clause has no overt subject.
    Predicate,
    /// Inside a prepositional phrase.
    PrepositionalPhrase,
    /// The sentence-final punctuation that supplied
    /// [`SentenceAnalysis::terminator`]. At most one word carries this role,
    /// and only when the terminator came from the sentence itself rather than
    /// from [`analyze_with_terminator`].
    Terminator,
}

impl Role {
    /// Every role, in declaration order.
    pub const ALL: [Self; 4] = [
        Self::Subject,
        Self::Predicate,
        Self::PrepositionalPhrase,
        Self::Terminator,
    ];

    /// A short, stable name.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Subject => "subject",
            Self::Predicate => "predicate",
            Self::PrepositionalPhrase => "prepositional phrase",
            Self::Terminator => "terminator",
        }
    }
}

impl fmt::Display for Role {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// A subject the clause does not spell out.
///
/// English imperative clauses have no overt subject; the understood subject is
/// the second-person pronoun (Quirk, Greenbaum, Leech & Svartvik, *A
/// Comprehensive Grammar of the English Language*, 1985, §11.24).
///
/// # No word is inserted into your sentence
///
/// This is a *report*, not an edit. [`analyze`] borrows the sentence it is
/// given and returns the implied subject as a value; it does not append a
/// synthetic word, so re-analysing the same slice always produces the same
/// answer and the caller's data is never rewritten.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ImpliedSubject {
    /// The understood *you* of an imperative.
    SecondPerson,
}

impl ImpliedSubject {
    /// The pronoun this subject stands for, in its citation form.
    ///
    /// Lowercase: it is a lexical citation, not a rendering of sentence-initial
    /// text. A caller inserting it into prose decides its capitalisation.
    #[must_use]
    pub const fn pronoun(self) -> &'static str {
        match self {
            Self::SecondPerson => "you",
        }
    }
}

impl fmt::Display for ImpliedSubject {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.pronoun())
    }
}

/// The result of analysing one sentence.
///
/// Borrows the sentence it describes, so every index and range it reports is an
/// index into that same slice, and no token is copied. Obtain one from
/// [`analyze`] or [`analyze_with_terminator`].
///
/// # Choosing the right accessor
///
/// | Need | Accessor | Allocates |
/// |---|---|---|
/// | Which part each word belongs to | [`roles`](Self::roles), [`role`](Self::role) | nothing |
/// | Iterate the subject or predicate | [`subject_words`](Self::subject_words), [`predicate_words`](Self::predicate_words) | nothing |
/// | Just the tokens, lazily | [`subject_tokens`](Self::subject_tokens), [`predicate_tokens`](Self::predicate_tokens) | nothing |
/// | A ready-made string | [`subject_to_string`](Self::subject_to_string), [`predicate_to_string`](Self::predicate_to_string) | one `String` |
/// | Phrase boundaries | [`prepositional_phrases`](Self::prepositional_phrases) | nothing |
/// | The classification | [`sentence_type`](Self::sentence_type), [`implied_subject`](Self::implied_subject) | nothing |
///
/// The `*_tokens` iterators and the `*_to_string` methods answer the same
/// question at different costs. Prefer the iterator: it borrows straight out of
/// the input and allocates nothing, so a caller that only counts, searches or
/// re-joins the words pays no allocation at all. Reach for `*_to_string` when
/// you genuinely need one owned, space-separated `String` — logging, a display
/// field, a key — and accept its single allocation. Neither is faster in a way
/// that should decide the choice for a program that needs the `String` anyway;
/// what decides it is whether the `String` is the thing you wanted.
///
/// ```
/// use verbora_analyzers::{TaggedWord as W, analyze};
///
/// let sentence = [
///     W::new("The", "DT"),
///     W::new("bear", "NN"),
///     W::new("chased", "VBD"),
///     W::new("the", "DT"),
///     W::new("squirrel", "NN"),
///     W::new(".", "."),
/// ];
/// let analysis = analyze(&sentence);
///
/// // Lazy: no allocation.
/// assert_eq!(analysis.subject_tokens().collect::<Vec<_>>(), ["The", "bear"]);
/// // Owned: one String.
/// assert_eq!(analysis.predicate_to_string(), "chased the squirrel");
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SentenceAnalysis<'w> {
    sentence: &'w [TaggedWord<'w>],
    roles: Vec<Role>,
    phrases: Vec<Range<usize>>,
    terminator: Option<Terminator>,
    terminator_index: Option<usize>,
    implied_subject: Option<ImpliedSubject>,
    sentence_type: Option<SentenceType>,
}

impl<'w> SentenceAnalysis<'w> {
    /// The sentence this analysis describes.
    #[must_use]
    pub const fn sentence(&self) -> &'w [TaggedWord<'w>] {
        self.sentence
    }

    /// One role per word, in sentence order. Always the same length as
    /// [`Self::sentence`].
    #[must_use]
    pub fn roles(&self) -> &[Role] {
        &self.roles
    }

    /// The role of the word at `index`, or `None` when the index is past the
    /// end of the sentence.
    #[must_use]
    pub fn role(&self, index: usize) -> Option<Role> {
        self.roles.get(index).copied()
    }

    /// The prepositional phrases, as half-open index ranges into
    /// [`Self::sentence`], in increasing order.
    ///
    /// Ranges never overlap, and never include the terminator. Each runs from
    /// its `IN` word through the first following noun **inclusive**, or to the
    /// end of the body when no noun follows.
    ///
    /// ```
    /// use verbora_analyzers::{TaggedWord as W, analyze};
    ///
    /// let sentence = [
    ///     W::new("the", "DT"),
    ///     W::new("cat", "NN"),
    ///     W::new("in", "IN"),
    ///     W::new("the", "DT"),
    ///     W::new("hats", "NNS"),
    /// ];
    /// let analysis = analyze(&sentence);
    /// assert_eq!(analysis.prepositional_phrases(), [2..5]);
    ///
    /// let phrase = &sentence[analysis.prepositional_phrases()[0].clone()];
    /// let tokens: Vec<&str> = phrase.iter().map(|w| w.token()).collect();
    /// assert_eq!(tokens, ["in", "the", "hats"]);
    /// ```
    #[must_use]
    pub fn prepositional_phrases(&self) -> &[Range<usize>] {
        &self.phrases
    }

    /// The terminal punctuation this sentence ends with, if any.
    #[must_use]
    pub const fn terminator(&self) -> Option<Terminator> {
        self.terminator
    }

    /// The index of the word that supplied [`Self::terminator`].
    ///
    /// `None` when the sentence has no terminator, **and** when the terminator
    /// was supplied out of band through [`analyze_with_terminator`] rather than
    /// found in the sentence. Distinguishing the two is what
    /// [`Self::terminator`] is for.
    #[must_use]
    pub const fn terminator_index(&self) -> Option<usize> {
        self.terminator_index
    }

    /// The understood subject of an imperative clause, if the clause is one.
    #[must_use]
    pub const fn implied_subject(&self) -> Option<ImpliedSubject> {
        self.implied_subject
    }

    /// The clause type, or `None` when no rule found evidence for one.
    #[must_use]
    pub const fn sentence_type(&self) -> Option<SentenceType> {
        self.sentence_type
    }

    /// The words with a given role, in sentence order.
    pub fn words_with_role(&self, role: Role) -> impl Iterator<Item = &'w TaggedWord<'w>> + Clone {
        self.roles
            .iter()
            .zip(self.sentence)
            .filter(move |(r, _)| **r == role)
            .map(|(_, word)| word)
    }

    /// The subject's words, in sentence order. Empty for an imperative clause.
    pub fn subject_words(&self) -> impl Iterator<Item = &'w TaggedWord<'w>> + Clone {
        self.words_with_role(Role::Subject)
    }

    /// The predicate's words, in sentence order.
    pub fn predicate_words(&self) -> impl Iterator<Item = &'w TaggedWord<'w>> + Clone {
        self.words_with_role(Role::Predicate)
    }

    /// The subject's tokens, in sentence order. Allocates nothing.
    pub fn subject_tokens(&self) -> impl Iterator<Item = &'w str> + Clone {
        self.subject_words().map(TaggedWord::token)
    }

    /// The predicate's tokens, in sentence order. Allocates nothing.
    pub fn predicate_tokens(&self) -> impl Iterator<Item = &'w str> + Clone {
        self.predicate_words().map(TaggedWord::token)
    }

    /// The subject's tokens joined by one U+0020 SPACE each. One allocation.
    ///
    /// No separator is added before the first token or after the last, and no
    /// empty slot is emitted for a word of another role — the result of an
    /// empty subject is the empty string, not a run of spaces.
    #[must_use]
    pub fn subject_to_string(&self) -> String {
        join(self.subject_tokens())
    }

    /// The predicate's tokens joined by one U+0020 SPACE each. One allocation.
    /// See [`Self::subject_to_string`].
    #[must_use]
    pub fn predicate_to_string(&self) -> String {
        join(self.predicate_tokens())
    }
}

/// Joins tokens with a single space, reserving the exact final length.
fn join<'s>(tokens: impl Iterator<Item = &'s str> + Clone) -> String {
    let bytes: usize = tokens.clone().map(str::len).sum();
    let count = tokens.clone().count();
    let mut out = String::with_capacity(bytes + count.saturating_sub(1));
    for (i, token) in tokens.enumerate() {
        if i > 0 {
            out.push(' ');
        }
        out.push_str(token);
    }
    out
}

/// Analyses a POS-tagged sentence, taking its terminator from the sentence
/// itself.
///
/// The last word is the terminator when its **token** is one of the scalars
/// [`Terminator`] specifies; its tag is not consulted, because taggers disagree
/// about how to tag punctuation while the token itself is unambiguous. When the
/// last word is not a terminator, the sentence has none and every word is part
/// of the body.
///
/// The sentence is borrowed and never modified: the returned
/// [`SentenceAnalysis`] reports what it found, and analysing the same slice
/// twice gives equal results.
///
/// # Choosing between `analyze` and `analyze_with_terminator`
///
/// | Situation | Call |
/// |---|---|
/// | Your tokenizer keeps terminal punctuation as a token | [`analyze`] |
/// | Your tokenizer strips punctuation, and you know what it stripped | [`analyze_with_terminator(s, Some(t))`](analyze_with_terminator) |
/// | You want the last word treated as an ordinary word whatever it looks like | [`analyze_with_terminator(s, None)`](analyze_with_terminator) |
///
/// [`analyze`] is the right call for the large majority of programs: a
/// tokenizer that keeps punctuation needs nothing else, and it is the only one
/// of the three that can report [`SentenceAnalysis::terminator_index`].
///
/// # Cost
///
/// A constant number of linear passes over the sentence — at most six, none
/// nested — and no work that depends on token length. Two allocations: one
/// `Vec<Role>` of exactly `sentence.len()` elements, and one
/// `Vec<Range<usize>>` sized to the number of prepositional phrases (never
/// allocated when there are none). **Not yet benchmarked** against this
/// pipeline; `benches/analyzers.rs` measures it, but no run exists, so this
/// crate publishes no timing figures.
///
/// ```
/// use verbora_analyzers::{ImpliedSubject, Role, SentenceType, TaggedWord as W, analyze};
///
/// let sentence = [
///     W::new("Vote", "VB"),
///     W::new("for", "IN"),
///     W::new("me", "PRP"),
///     W::new("!", "."),
/// ];
/// let analysis = analyze(&sentence);
///
/// assert_eq!(analysis.sentence_type(), Some(SentenceType::Imperative));
/// assert_eq!(analysis.implied_subject(), Some(ImpliedSubject::SecondPerson));
/// assert_eq!(
///     analysis.roles(),
///     [
///         Role::Predicate,
///         Role::PrepositionalPhrase,
///         Role::PrepositionalPhrase,
///         Role::Terminator,
///     ]
/// );
/// // The input is untouched, so a second analysis agrees with the first.
/// assert_eq!(analyze(&sentence), analysis);
/// ```
#[must_use]
pub fn analyze<'w>(sentence: &'w [TaggedWord<'w>]) -> SentenceAnalysis<'w> {
    let found = sentence.len().checked_sub(1).and_then(|index| {
        let kind = Terminator::from_token(sentence.get(index)?.token())?;
        Some((kind, index))
    });
    let (terminator, terminator_index) = match found {
        Some((kind, index)) => (Some(kind), Some(index)),
        None => (None, None),
    };
    build(sentence, terminator, terminator_index)
}

/// Analyses a POS-tagged sentence with the terminator supplied out of band.
///
/// No word is ever treated as terminal punctuation: the whole slice is the
/// body, [`SentenceAnalysis::terminator_index`] is always `None`, and no word
/// receives [`Role::Terminator`]. Pass `Some(kind)` when your tokenizer
/// stripped the mark and you know which it was, and `None` when the sentence
/// genuinely has no terminator.
///
/// See [`analyze`] for how to choose between the two, and for the cost.
///
/// ```
/// use verbora_analyzers::{Role, SentenceType, TaggedWord as W, Terminator, analyze_with_terminator};
///
/// // Punctuation stripped by the tokenizer, supplied here instead.
/// let stripped = [W::new("Who", "WP"), W::new("voted", "VBD")];
/// let analysis = analyze_with_terminator(&stripped, Some(Terminator::Question));
/// assert_eq!(analysis.sentence_type(), Some(SentenceType::Interrogative));
/// assert_eq!(analysis.terminator_index(), None);
///
/// // `None` keeps a trailing full stop as an ordinary word.
/// let kept = [W::new("Who", "WP"), W::new("voted", "VBD"), W::new(".", ".")];
/// let analysis = analyze_with_terminator(&kept, None);
/// assert_eq!(analysis.role(2), Some(Role::Predicate));
/// // No terminator, so the wh-word decides instead.
/// assert_eq!(analysis.sentence_type(), Some(SentenceType::Interrogative));
/// ```
#[must_use]
pub fn analyze_with_terminator<'w>(
    sentence: &'w [TaggedWord<'w>],
    terminator: Option<Terminator>,
) -> SentenceAnalysis<'w> {
    build(sentence, terminator, None)
}

/// The pipeline both entry points share. `terminator_index`, when present, is
/// the index of a word of `sentence` that supplied `terminator`.
fn build<'w>(
    sentence: &'w [TaggedWord<'w>],
    terminator: Option<Terminator>,
    terminator_index: Option<usize>,
) -> SentenceAnalysis<'w> {
    let body_len = terminator_index.unwrap_or(sentence.len());
    let body = sentence.get(..body_len).unwrap_or(sentence);

    let phrases = prepositional_phrases(body);
    let mut inside = vec![false; body.len()];
    for phrase in &phrases {
        for slot in inside.get_mut(phrase.clone()).unwrap_or_default() {
            *slot = true;
        }
    }

    let imperative = is_imperative(body);
    let predicate_start = if imperative {
        Some(0)
    } else {
        (0..body.len()).find(|&i| {
            !inside.get(i).copied().unwrap_or(false)
                && body.get(i).is_some_and(|w| w.tag_class().is_verb())
        })
    };

    let mut roles = Vec::with_capacity(sentence.len());
    for i in 0..body.len() {
        roles.push(if inside.get(i).copied().unwrap_or(false) {
            Role::PrepositionalPhrase
        } else if predicate_start.is_some_and(|start| i >= start) {
            Role::Predicate
        } else {
            Role::Subject
        });
    }
    if terminator_index.is_some() {
        roles.push(Role::Terminator);
    }

    SentenceAnalysis {
        sentence,
        roles,
        phrases,
        terminator,
        terminator_index,
        implied_subject: imperative.then_some(ImpliedSubject::SecondPerson),
        sentence_type: classify(body, terminator, imperative),
    }
}

/// Stage 1: the prepositional phrases of the body, in increasing order.
///
/// A word tagged `IN` opens a phrase unless one is already open; the first
/// following noun closes it, inclusive. An unterminated phrase runs to the end
/// of the body.
fn prepositional_phrases(body: &[TaggedWord<'_>]) -> Vec<Range<usize>> {
    let mut phrases = Vec::new();
    let mut open: Option<usize> = None;
    for (i, word) in body.iter().enumerate() {
        match word.tag_class() {
            TagClass::Preposition if open.is_none() => open = Some(i),
            TagClass::Noun => {
                if let Some(start) = open.take() {
                    phrases.push(start..i + 1);
                }
            }
            _ => {}
        }
    }
    if let Some(start) = open {
        phrases.push(start..body.len());
    }
    phrases
}

/// Stage 2: whether the body is an imperative clause.
///
/// True when the first word that is neither an adverb nor an interjection is
/// tagged `VB`.
fn is_imperative(body: &[TaggedWord<'_>]) -> bool {
    body.iter()
        .find(|word| !word.tag_class().precedes_imperative_verb())
        .is_some_and(|word| word.tag_class() == TagClass::BaseVerb)
}

/// Stage 4: the clause type.
fn classify(
    body: &[TaggedWord<'_>],
    terminator: Option<Terminator>,
    imperative: bool,
) -> Option<SentenceType> {
    match terminator {
        Some(Terminator::Question) => Some(SentenceType::Interrogative),
        Some(Terminator::Exclamation) => Some(if imperative {
            SentenceType::Imperative
        } else {
            SentenceType::Exclamative
        }),
        Some(Terminator::FullStop) => Some(if imperative {
            SentenceType::Imperative
        } else {
            SentenceType::Declarative
        }),
        None => {
            if imperative {
                Some(SentenceType::Imperative)
            } else if is_wh_initial(body) || is_inverted(body) || ends_with_tag_question(body) {
                Some(SentenceType::Interrogative)
            } else {
                None
            }
        }
    }
}

/// A wh-word in first position — *Who voted*, *Which bear ran*.
fn is_wh_initial(body: &[TaggedWord<'_>]) -> bool {
    body.first()
        .is_some_and(|word| word.tag_class() == TagClass::WhWord)
}

/// Subject–operator inversion: a finite verb in first position, which an
/// English declarative or imperative clause cannot have.
fn is_inverted(body: &[TaggedWord<'_>]) -> bool {
    body.first()
        .is_some_and(|word| word.tag_class() == TagClass::FiniteVerb)
}

/// A trailing tag question: a personal pronoun preceded, past any adverbs, by a
/// finite verb — *…, isn't it*, *…, should we*.
fn ends_with_tag_question(body: &[TaggedWord<'_>]) -> bool {
    let mut index = match body.len().checked_sub(1) {
        Some(index) => index,
        None => return false,
    };
    if body.get(index).map(TaggedWord::tag_class) != Some(TagClass::Pronoun) {
        return false;
    }
    while index > 0 {
        index -= 1;
        match body.get(index).map(TaggedWord::tag_class) {
            Some(TagClass::Adverb) => {}
            Some(TagClass::FiniteVerb) => return true,
            _ => return false,
        }
    }
    false
}
