//! The 36 rule templates: predicate functions, windows, and parameter sources.
//!
//! This is `lib/RuleTemplates`, transcribed rather than tidied. Nine of the
//! entries are wrong in ways that are *observable*, and every one of them has to
//! survive the port:
//!
//! | Template | What is wrong | Effect if "fixed" |
//! |---|---|---|
//! | `CURRENT-WORD-ENDS-WITH` | uses `indexOf`, i.e. **first** occurrence | `"sees"` would end with `"s"`; four of the eighteen English rules change |
//! | `WDAND2TAGAFT` | reads `taggedWords[i+2][1]`, always `undefined` | two Dutch rules stop being inert |
//! | `CURRENT-WORD-IS-CAP` | ignores its `YES`/`NO` parameter entirely | `NP NN CURRENT-WORD-IS-CAP NO` fires on capitalised words |
//! | `PREV2TAG` | declares two parameters, reads one | second parameter would start mattering |
//! | `NEXT1OR2WD` | declares one parameter, reads two | second branch would stop being dead |
//! | `NEXTBIGRAM` / `PREVBIGRAM` | compare **tokens**, but the Dutch data passes tags | six Dutch rules stop being inert |
//! | `currentWordParameterValues` | indexes the `Sentence` object, not `.taggedWords` | nine templates become trainable that cannot be trained in the reference |
//! | `prev1Or2WordParameterValues` | same bug | `PREV1OR2WD` becomes trainable |
//! | `currentWordEndsWithParameterValues` | always returns `["ing"]` | suffix training would generalise |
//!
//! Parameter *order* is also inconsistent between the two-parameter templates
//! and is transcribed literally: `WDNEXTTAG` takes (token, next tag) while
//! `WDPREVTAG` takes (**prev tag**, token). Normalising them silently mis-tags
//! Dutch text.

use std::borrow::Cow;

use crate::error::TaggerError;
use crate::numfmt;
use crate::sentence::{Tag, TaggedWord};
use crate::utf16;

/// What a predicate returned.
///
/// The reference passes predicate results straight into `if (...)`, so the return
/// type is genuinely `boolean | number | undefined`: `currentWordIsNumber`
/// returns `parseFloat`'s result and `nextWordIs` falls off the end of the
/// function at the last position. Modelling it as `bool` would merge
/// `parseFloat("0abc") === 0` (falsy) with `parseFloat("5abc") === 5` (truthy)
/// only by accident, and would lose the distinction the fixture records.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PredValue {
    /// A boolean.
    Bool(bool),
    /// A number, from `currentWordIsNumber`'s `parseFloat` fallback.
    Num(f64),
    /// `undefined`, from `nextWordIs` at the last position.
    Undefined,
}

impl PredValue {
    /// The reference truthiness: `0`, `NaN`, `false` and `undefined` are falsy.
    #[inline]
    #[must_use]
    pub fn is_truthy(self) -> bool {
        match self {
            Self::Bool(b) => b,
            Self::Num(n) => n != 0.0 && !n.is_nan(),
            Self::Undefined => false,
        }
    }
}

/// Which predicate function a template uses.
///
/// Resolved once, when a rule is built, so the tagging loop is a `match` on a
/// one-byte discriminant instead of a string-keyed hash lookup and an indirect
/// call — the reference does the lookup per `new Predicate`, then an indirect call
/// per evaluation, of which there are (positions × rules).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum PredicateKind {
    /// `NEXT-TAG`, `NEXT-WORD-IS-TAG`, `NEXTTAG`.
    NextTagIs,
    /// `NEXT2TAG`.
    Next2TagIs,
    /// `NEXT1OR2TAG`.
    Next1Or2TagIs,
    /// `NEXT1OR2OR3TAG`.
    Next1Or2Or3Tag,
    /// `PREV-TAG`, `PREVTAG`.
    PrevTagIs,
    /// `PREV2TAG` — reads only parameter 1 despite declaring two.
    Prev2TagIs,
    /// `PREV-1-OR-2-TAG`, `PREV1OR2TAG`.
    Prev1Or2Tag,
    /// `PREV-1-OR-2-OR-3-TAG`, `PREV1OR2OR3TAG`.
    Prev1Or2Or3Tag,
    /// `SURROUNDTAG`.
    SurroundedByTags,
    /// `NEXTWD` — returns `undefined`, not `false`, at the last position.
    NextWordIs,
    /// `NEXT1OR2WD` — reads two parameters despite declaring one.
    Next1Or2WordIs,
    /// `PREV-WORD-IS`, `PREVWD` — case-insensitive on both sides.
    PrevWordIs,
    /// `PREV1OR2WD` — case-insensitive on both sides.
    Prev1Or2WordIs,
    /// `CURWD`.
    CurrentWordIs,
    /// `CURRENT-WORD-IS-CAP` — ignores its parameter.
    CurrentWordIsCap,
    /// `NEXT-WORD-IS-CAP`.
    NextWordIsCap,
    /// `PREV-WORD-IS-CAP`.
    PrevWordIsCap,
    /// `CURRENT-WORD-IS-NUMBER`.
    CurrentWordIsNumber,
    /// `CURRENT-WORD-IS-URL`.
    CurrentWordIsUrl,
    /// `CURRENT-WORD-ENDS-WITH` — first-occurrence test, not a suffix test.
    CurrentWordEndsWith,
    /// `WDNEXTTAG` — (current token, next tag).
    CurrentWordAndNextTagAre,
    /// `WDPREVTAG` — (**previous tag**, current token).
    CurrentWordAndPrevTagAre,
    /// `WDAND2TAGBFR` — (**tag at i-2**, current token).
    CurrentWordAnd2TagBeforeAre,
    /// `WDAND2TAGAFT` — permanently false; see the module docs.
    CurrentWordAnd2TagAfterAre,
    /// `WDAND2AFT` — (current token, token at i+2).
    CurrentWordAnd2AfterAre,
    /// `RBIGRAM` — (current token, next token).
    RightBigramIs,
    /// `LBIGRAM` — (previous token, current token).
    LeftBigramIs,
    /// `NEXTBIGRAM` — compares tokens at i+1 and i+2.
    NextBigramIs,
    /// `PREVBIGRAM` — compares tokens at i-2 and i-1.
    PrevBigramIs,
    /// `DEFAULT`, and every unrecognised predicate name: always false.
    Default,
    /// A name that collides with an `Object.prototype` member.
    ///
    /// `ruleTemplates['toString']` finds the inherited function, so
    /// `if (!this.meta) this.meta = ruleTemplates.DEFAULT` never fires and
    /// `this.meta.function` is `undefined`: evaluating throws. There are twelve
    /// such names, `__proto__` included.
    PrototypeLeak,
}

/// Where a template's parameter values come from during training.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParamSource {
    /// `[tag(i+1)]` if in range.
    NextTag,
    /// `[tag(i+2)]` if in range.
    Next2Tag,
    /// `tag(i+1)` then `tag(i+2)`, each if in range.
    Next1Or2Tag,
    /// `Next1Or2Tag` plus `tag(i+3)`.
    Next1Or2Or3Tag,
    /// `[tag(i-1)]` if in range.
    PrevTag,
    /// `tag(i-1)` then `tag(i-2)`.
    Prev1Or2Tag,
    /// `Prev1Or2Tag` plus `tag(i-3)`.
    Prev1Or2Or3Tag,
    /// `[tag(i+2)]` if in range.
    TwoTagAfter,
    /// `[tag(i-2)]` if in range.
    TwoTagBefore,
    /// `[token(i+1)]` if in range.
    NextWord,
    /// `[token(i-1)]` if in range.
    PrevWord,
    /// `[token(i+2)]` if in range.
    TwoWordAfter,
    /// `[token(i-2)]` if in range.
    TwoWordBefore,
    /// `token(i+1)` then `token(i+2)`.
    Next1Or2Word,
    /// The hard-coded `["ing"]`, whatever the sentence says.
    EndsWithConstant,
    /// **Throws.** `currentWordParameterValues` reads `sentence[i].token`,
    /// indexing the `Sentence` object rather than its `taggedWords` array.
    CurrentWordBroken,
    /// **Throws.** `prev1Or2WordParameterValues` has the same bug.
    Prev1Or2WordBroken,
}

/// One entry of the `ruleTemplates` table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Template {
    /// The name rules refer to it by.
    pub name: &'static str,
    /// The predicate it evaluates.
    pub kind: PredicateKind,
    /// `[left, right]` offsets that must be inside the sentence for the template
    /// to be applicable at a site. Note these do **not** bound what the
    /// predicate reads — `PREV1OR2OR3TAG` declares `[-1, 0]` but looks back
    /// three positions.
    pub window: [i32; 2],
    /// The declared parameter count. Three templates contradict their own
    /// functions here; see the module docs.
    pub nr_parameters: u8,
    /// Source of candidate values for parameter 1 during training.
    pub parameter1_values: Option<ParamSource>,
    /// Source of candidate values for parameter 2 during training.
    pub parameter2_values: Option<ParamSource>,
}

macro_rules! templates {
    ($($name:literal => $kind:ident, $w:expr, $n:literal, $p1:expr, $p2:expr;)*) => {
        /// The rule-template table, in the reference insertion order.
        ///
        /// Order is observable: `Object.keys(ruleTemplates)` is part of the
        /// recorded fixture, and the trainer iterates whatever template list it
        /// is given in order.
        pub static TEMPLATES: &[Template] = &[$(Template {
            name: $name,
            kind: PredicateKind::$kind,
            window: $w,
            nr_parameters: $n,
            parameter1_values: $p1,
            parameter2_values: $p2,
        }),*];
    };
}

use ParamSource as P;

templates! {
    "NEXT-TAG"             => NextTagIs,                   [0, 1],  1, Some(P::NextTag),           None;
    "NEXT-WORD-IS-CAP"     => NextWordIsCap,               [0, 1],  0, None,                       None;
    "PREV-1-OR-2-OR-3-TAG" => Prev1Or2Or3Tag,              [-1, 0], 1, Some(P::Prev1Or2Or3Tag),    None;
    "PREV-1-OR-2-TAG"      => Prev1Or2Tag,                 [-1, 0], 1, Some(P::Prev1Or2Tag),       None;
    "NEXT-WORD-IS-TAG"     => NextTagIs,                   [0, 1],  1, Some(P::NextTag),           None;
    "PREV-TAG"             => PrevTagIs,                   [-1, 0], 1, Some(P::PrevTag),           None;
    "PREV-WORD-IS-CAP"     => PrevWordIsCap,               [-1, 0], 0, None,                       None;
    "CURRENT-WORD-IS-CAP"  => CurrentWordIsCap,            [0, 0],  0, None,                       None;
    "CURRENT-WORD-IS-NUMBER" => CurrentWordIsNumber,       [0, 0],  0, None,                       None;
    "CURRENT-WORD-IS-URL"  => CurrentWordIsUrl,            [0, 0],  0, None,                       None;
    "CURRENT-WORD-ENDS-WITH" => CurrentWordEndsWith,       [0, 0],  1, Some(P::EndsWithConstant),  None;
    "PREV-WORD-IS"         => PrevWordIs,                  [-1, 0], 1, Some(P::PrevWord),          None;
    "PREVTAG"              => PrevTagIs,                   [-1, 0], 1, Some(P::PrevTag),           None;
    "NEXT1OR2TAG"          => Next1Or2TagIs,               [0, 1],  1, Some(P::Next1Or2Tag),       None;
    "NEXTTAG"              => NextTagIs,                   [0, 1],  1, Some(P::NextTag),           None;
    "PREV1OR2TAG"          => Prev1Or2Tag,                 [-1, 0], 1, Some(P::Prev1Or2Tag),       None;
    "WDAND2TAGAFT"         => CurrentWordAnd2TagAfterAre,  [0, 2],  2, Some(P::CurrentWordBroken), Some(P::TwoTagAfter);
    "NEXT1OR2OR3TAG"       => Next1Or2Or3Tag,              [0, 1],  1, Some(P::Next1Or2Or3Tag),    None;
    "CURWD"                => CurrentWordIs,               [0, 0],  1, Some(P::CurrentWordBroken), None;
    "SURROUNDTAG"          => SurroundedByTags,            [-1, 1], 2, Some(P::PrevTag),           Some(P::NextTag);
    "PREV1OR2OR3TAG"       => Prev1Or2Or3Tag,              [-1, 0], 1, Some(P::Prev1Or2Or3Tag),    None;
    "WDNEXTTAG"            => CurrentWordAndNextTagAre,    [0, 1],  2, Some(P::CurrentWordBroken), Some(P::NextTag);
    "PREV1OR2WD"           => Prev1Or2WordIs,              [-1, 0], 1, Some(P::Prev1Or2WordBroken), None;
    "NEXTWD"               => NextWordIs,                  [0, 1],  1, Some(P::NextWord),          None;
    "PREVWD"               => PrevWordIs,                  [-1, 0], 1, Some(P::PrevWord),          None;
    "NEXT2TAG"             => Next2TagIs,                  [0, 2],  1, Some(P::Next2Tag),          None;
    "WDAND2TAGBFR"         => CurrentWordAnd2TagBeforeAre, [-2, 0], 2, Some(P::CurrentWordBroken), Some(P::TwoTagBefore);
    "WDAND2AFT"            => CurrentWordAnd2AfterAre,     [0, 2],  2, Some(P::CurrentWordBroken), Some(P::TwoTagAfter);
    "WDPREVTAG"            => CurrentWordAndPrevTagAre,    [-1, 0], 2, Some(P::CurrentWordBroken), Some(P::PrevTag);
    "RBIGRAM"              => RightBigramIs,               [0, 1],  2, Some(P::CurrentWordBroken), Some(P::NextWord);
    "LBIGRAM"              => LeftBigramIs,                [-1, 0], 2, Some(P::PrevWord),          Some(P::CurrentWordBroken);
    "NEXTBIGRAM"           => NextBigramIs,                [0, 2],  2, Some(P::NextWord),          Some(P::TwoWordAfter);
    "PREVBIGRAM"           => PrevBigramIs,                [-2, 0], 2, Some(P::TwoWordBefore),     Some(P::PrevWord);
    "PREV2TAG"             => Prev2TagIs,                  [-2, 0], 2, Some(P::TwoTagBefore),      Some(P::PrevTag);
    "NEXT1OR2WD"           => Next1Or2WordIs,              [0, 1],  1, Some(P::Next1Or2Word),      None;
    "DEFAULT"              => Default,                     [0, 0],  0, None,                       None;
}

/// The twelve own property names of `Object.prototype`.
///
/// A predicate named after any of them resolves `ruleTemplates[name]` through
/// the prototype chain to a `Function` (or, for `__proto__`, to
/// `Object.prototype` itself). Both are truthy, so the `DEFAULT` fallback is
/// skipped and evaluation throws. A Rust `HashMap` has no prototype chain, so
/// this list is the only way to reproduce it.
pub static OBJECT_PROTOTYPE_MEMBERS: &[&str] = &[
    "constructor",
    "__defineGetter__",
    "__defineSetter__",
    "hasOwnProperty",
    "__lookupGetter__",
    "__lookupSetter__",
    "isPrototypeOf",
    "propertyIsEnumerable",
    "toString",
    "valueOf",
    "__proto__",
    "toLocaleString",
];

/// Looks a template up by name, exactly as `ruleTemplates[name]` does.
#[must_use]
pub fn template(name: &str) -> Option<&'static Template> {
    TEMPLATES.iter().find(|t| t.name == name)
}

/// Resolves a predicate name to the kind that will be evaluated.
///
/// Mirrors `new Predicate(name)`: a known template wins; an `Object.prototype`
/// member name yields the poisoned [`PredicateKind::PrototypeLeak`]; anything
/// else falls back to `DEFAULT`.
#[must_use]
pub fn resolve(name: &str) -> PredicateKind {
    if let Some(t) = template(name) {
        return t.kind;
    }
    if OBJECT_PROTOTYPE_MEMBERS.contains(&name) {
        return PredicateKind::PrototypeLeak;
    }
    PredicateKind::Default
}

// ---------------------------------------------------------------------------
// Accessors
//
// The reference indexes arrays with a Number, so out-of-range and negative indices
// are meaningful: they read `undefined`, which is harmless where a bounds check
// guards the access and a TypeError where one does not. `isize` reproduces that
// faithfully; `usize` would make `i - 1` panic or wrap.
// ---------------------------------------------------------------------------

#[inline]
fn at<'w, 'a>(words: &'w [TaggedWord<'a>], i: isize) -> Option<&'w TaggedWord<'a>> {
    usize::try_from(i).ok().and_then(|i| words.get(i))
}

/// `taggedWords[i].tag`, throwing when the element does not exist.
#[inline]
fn tag_at<'w>(words: &'w [TaggedWord<'_>], i: isize) -> Result<Option<&'w str>, TaggerError> {
    at(words, i)
        .map(|w| w.tag())
        .ok_or(TaggerError::undefined("tag"))
}

/// `taggedWords[i].token`, throwing when the element does not exist.
#[inline]
fn token_at<'w>(words: &'w [TaggedWord<'_>], i: isize) -> Result<&'w str, TaggerError> {
    at(words, i)
        .map(|w| &*w.token)
        .ok_or(TaggerError::undefined("token"))
}

/// `word[0] === word[0].toUpperCase()`, throwing on an empty token.
#[inline]
fn is_cap(word: &str) -> Result<bool, TaggerError> {
    utf16::first_char_is_own_uppercase(word).ok_or(TaggerError::undefined("toUpperCase"))
}

/// `a.toLowerCase() === b.toLowerCase()`, throwing when `b` is `undefined`.
///
/// Allocation-free when both sides are ASCII, which covers essentially all
/// real rule parameters; otherwise it falls back to full Unicode lowercasing,
/// which — unlike `to_ascii_lowercase` — handles Greek final sigma and the
/// Turkish dotted capital the way `String.prototype.toLowerCase` does.
#[inline]
fn eq_lowercased(a: &str, b: Option<&str>) -> Result<bool, TaggerError> {
    let b = b.ok_or(TaggerError::undefined("toLowerCase"))?;
    if a.is_ascii() && b.is_ascii() {
        return Ok(a.eq_ignore_ascii_case(b));
    }
    Ok(a.to_lowercase() == b.to_lowercase())
}

// ---------------------------------------------------------------------------
// Evaluation
// ---------------------------------------------------------------------------

/// Evaluates a predicate at `position`.
///
/// # Errors
///
/// Returns the `TypeError` the reference throws: reading a property of an
/// out-of-range element, capitalising an empty token, lowercasing an absent
/// parameter, or calling a prototype-leaked "predicate".
#[allow(clippy::too_many_lines)] // one arm per predicate; splitting hides the shape
pub fn evaluate(
    kind: PredicateKind,
    words: &[TaggedWord<'_>],
    position: isize,
    p1: Option<&str>,
    p2: Option<&str>,
) -> Result<PredValue, TaggerError> {
    use PredicateKind as K;
    let i = position;
    let len = words.len() as isize;
    let b = |v: bool| Ok(PredValue::Bool(v));

    match kind {
        K::NextTagIs => {
            if i < len - 1 {
                b(tag_at(words, i + 1)? == p1)
            } else {
                b(false)
            }
        }
        K::Next2TagIs => {
            if i < len - 2 {
                b(tag_at(words, i + 2)? == p1)
            } else {
                b(false)
            }
        }
        K::Next1Or2TagIs => b(next_1_or_2_tag(words, i, p1)?),
        K::Next1Or2Or3Tag => {
            // The reference evaluates the `next3` block *before* the delegated
            // call, and `||` short-circuits afterwards; the order is preserved
            // because either read can throw.
            let next3 = if i < len - 3 {
                tag_at(words, i + 3)? == p1
            } else {
                false
            };
            b(next_1_or_2_tag(words, i, p1)? || next3)
        }
        K::PrevTagIs => {
            let mut prev = false;
            if i > 0 {
                prev = tag_at(words, i - 1)? == p1;
            }
            b(prev)
        }
        K::Prev2TagIs => {
            // Declares two parameters; reads only the first.
            let mut prev2 = false;
            if i > 1 {
                prev2 = tag_at(words, i - 2)? == p1;
            }
            b(prev2)
        }
        K::Prev1Or2Tag => b(prev_1_or_2_tag(words, i, p1)?),
        K::Prev1Or2Or3Tag => {
            // `prev3` starts as the reference `null`, which is `!==` both a string
            // and `undefined` — so an out-of-range position never matches, even
            // for a rule whose parameter is `undefined`. `Option<Option<_>>`
            // models the three states: outer `None` is `null`.
            let mut prev3: Option<Option<&str>> = None;
            if i > 2 {
                prev3 = Some(tag_at(words, i - 3)?);
            }
            b(prev_1_or_2_tag(words, i, p1)? || prev3 == Some(p1))
        }
        K::SurroundedByTags => {
            if i < len - 1 && tag_at(words, i + 1)? == p2 {
                if i > 0 {
                    b(tag_at(words, i - 1)? == p1)
                } else {
                    b(false)
                }
            } else {
                b(false)
            }
        }
        K::NextWordIs => {
            // No `else` branch in the reference: the last position returns
            // `undefined`, which is falsy but not `false`.
            if i < len - 1 {
                b(Some(token_at(words, i + 1)?) == p1)
            } else {
                Ok(PredValue::Undefined)
            }
        }
        K::Next1Or2WordIs => {
            // Declares one parameter but reads two, so with a one-parameter rule
            // the second branch compares against `undefined` and never fires.
            let mut next1 = false;
            let mut next2 = false;
            if i < len - 1 {
                next1 = Some(token_at(words, i + 1)?) == p1;
            }
            if i < len - 2 {
                next2 = Some(token_at(words, i + 2)?) == p2;
            }
            b(next1 || next2)
        }
        K::PrevWordIs => {
            if i > 0 {
                b(eq_lowercased(token_at(words, i - 1)?, p1)?)
            } else {
                b(false)
            }
        }
        K::Prev1Or2WordIs => {
            let mut prev1 = false;
            let mut prev2 = false;
            if i > 0 {
                prev1 = eq_lowercased(token_at(words, i - 1)?, p1)?;
            }
            if i > 1 {
                prev2 = eq_lowercased(token_at(words, i - 2)?, p1)?;
            }
            b(prev1 || prev2)
        }
        K::CurrentWordIs => b(Some(token_at(words, i)?) == p1),
        K::CurrentWordIsCap => b(is_cap(token_at(words, i)?)?),
        K::NextWordIsCap => {
            if i < len - 1 {
                b(is_cap(token_at(words, i + 1)?)?)
            } else {
                b(false)
            }
        }
        K::PrevWordIsCap => {
            if i > 0 {
                b(is_cap(token_at(words, i - 1)?)?)
            } else {
                b(false)
            }
        }
        K::CurrentWordIsNumber => {
            let token = token_at(words, i)?;
            // `let isNumber = !isNaN(token); if (!isNumber) isNumber = parseFloat(token)`
            // — the variable changes type, and the caller sees the difference.
            if numfmt::is_nan(token) {
                let f = numfmt::parse_float(token);
                if p1 == Some("YES") {
                    Ok(PredValue::Num(f))
                } else {
                    b(!(f != 0.0 && !f.is_nan()))
                }
            } else if p1 == Some("YES") {
                b(true)
            } else {
                b(false)
            }
        }
        K::CurrentWordIsUrl => {
            let token = token_at(words, i)?;
            let is_url = token.contains('.') && utf16::has_two_adjacent_ascii_letters(token);
            b(if p1 == Some("YES") { is_url } else { !is_url })
        }
        K::CurrentWordEndsWith => {
            let word = token_at(words, i)?;
            // `!parameter` is falsy for `undefined` AND for the empty string.
            let Some(p) = p1.filter(|p| !p.is_empty()) else {
                return b(false);
            };
            let (wl, pl) = (utf16::len(word), utf16::len(p));
            if pl > wl {
                return b(false);
            }
            // NOT `ends_with`: `indexOf` finds the FIRST occurrence, so a suffix
            // that also appears earlier fails. "sees" does not end with "s".
            b(utf16::index_of(word, p) == Some(wl - pl))
        }
        K::CurrentWordAndNextTagAre => {
            let current = Some(token_at(words, i)?) == p1;
            let mut next_tag = false;
            if i < len - 1 {
                next_tag = tag_at(words, i + 1)? == p2;
            }
            b(current && next_tag)
        }
        K::CurrentWordAndPrevTagAre => {
            // Parameter order reversed relative to WDNEXTTAG: p1 is the tag.
            let current = Some(token_at(words, i)?) == p2;
            let mut prev_tag = false;
            if i > 0 {
                prev_tag = tag_at(words, i - 1)? == p1;
            }
            b(current && prev_tag)
        }
        K::CurrentWordAnd2TagBeforeAre => {
            let current = Some(token_at(words, i)?) == p2;
            let mut two_before = false;
            if i > 1 {
                two_before = tag_at(words, i - 2)? == p1;
            }
            b(current && two_before)
        }
        K::CurrentWordAnd2TagAfterAre => {
            // The reference reads `taggedWords[i + 2][1]` — integer index 1 on an
            // object — which is always `undefined`. Reproduced literally: the
            // comparison can only succeed when parameter 2 is itself absent, so
            // the two bundled Dutch rules using this template are inert.
            if i < len - 2 {
                let slot_1_is_undefined = p2.is_none();
                if slot_1_is_undefined {
                    b(Some(token_at(words, i)?) == p1)
                } else {
                    b(false)
                }
            } else {
                b(false)
            }
        }
        K::CurrentWordAnd2AfterAre => {
            let current = Some(token_at(words, i)?) == p1;
            let mut two_after = false;
            if i < len - 2 {
                two_after = Some(token_at(words, i + 2)?) == p2;
            }
            b(current && two_after)
        }
        K::RightBigramIs => {
            let word1 = Some(token_at(words, i)?) == p1;
            let mut word2 = false;
            if i < len - 1 {
                word2 = Some(token_at(words, i + 1)?) == p2;
            }
            b(word1 && word2)
        }
        K::LeftBigramIs => {
            let word2 = Some(token_at(words, i)?) == p2;
            let mut word1 = false;
            if i > 0 {
                word1 = Some(token_at(words, i - 1)?) == p1;
            }
            b(word1 && word2)
        }
        K::NextBigramIs => {
            // Compares TOKENS, though the five bundled Dutch NEXTBIGRAM rules
            // pass tag strings — so those rules can never fire. Kept dead.
            let mut word1 = false;
            let mut word2 = false;
            if i < len - 1 {
                word1 = Some(token_at(words, i + 1)?) == p1;
            }
            if i < len - 2 {
                word2 = Some(token_at(words, i + 2)?) == p2;
            }
            b(word1 && word2)
        }
        K::PrevBigramIs => {
            let mut word1 = false;
            let mut word2 = false;
            if i > 1 {
                word1 = Some(token_at(words, i - 2)?) == p1;
            }
            if i > 0 {
                word2 = Some(token_at(words, i - 1)?) == p2;
            }
            b(word1 && word2)
        }
        K::Default => b(false),
        K::PrototypeLeak => Err(TaggerError::PredicateNotAFunction),
    }
}

#[inline]
fn next_1_or_2_tag(
    words: &[TaggedWord<'_>],
    i: isize,
    p: Option<&str>,
) -> Result<bool, TaggerError> {
    let len = words.len() as isize;
    let mut next1 = false;
    let mut next2 = false;
    if i < len - 1 {
        next1 = tag_at(words, i + 1)? == p;
    }
    if i < len - 2 {
        next2 = tag_at(words, i + 2)? == p;
    }
    Ok(next1 || next2)
}

#[inline]
fn prev_1_or_2_tag(
    words: &[TaggedWord<'_>],
    i: isize,
    p: Option<&str>,
) -> Result<bool, TaggerError> {
    // Both sentinels start as the reference `null`, not `false`.
    let mut prev1: Option<Option<&str>> = None;
    let mut prev2: Option<Option<&str>> = None;
    if i > 0 {
        prev1 = Some(tag_at(words, i - 1)?);
    }
    if i > 1 {
        prev2 = Some(tag_at(words, i - 2)?);
    }
    Ok(prev1 == Some(p) || prev2 == Some(p))
}

// ---------------------------------------------------------------------------
// Parameter value generation (training only)
// ---------------------------------------------------------------------------

/// Appends the candidate parameter values a template offers at a site.
///
/// # Errors
///
/// Two of the seventeen sources throw: `currentWordParameterValues` and
/// `prev1Or2WordParameterValues` index the `Sentence` object instead of its
/// `taggedWords` array, so any training run that selects one of the nine
/// templates using them crashes. A working Rust version would make those nine
/// templates trainable and produce rules the reference can never produce, so the
/// bug is reproduced rather than repaired.
pub fn parameter_values(
    source: ParamSource,
    words: &[TaggedWord<'_>],
    i: isize,
    out: &mut Vec<Option<Tag>>,
) -> Result<(), TaggerError> {
    let len = words.len() as isize;
    let tag = |i: isize| -> Option<Tag> { at(words, i).and_then(|w| w.tag.owned()) };
    let token = |i: isize| -> Option<Tag> {
        at(words, i).map(|w| Cow::Owned(w.token.clone().into_owned()))
    };
    match source {
        P::NextTag => {
            if i < len - 1 {
                out.push(tag(i + 1));
            }
        }
        P::Next2Tag => {
            if i < len - 2 {
                out.push(tag(i + 2));
            }
        }
        P::Next1Or2Tag => {
            if i < len - 1 {
                out.push(tag(i + 1));
            }
            if i < len - 2 {
                out.push(tag(i + 2));
            }
        }
        P::Next1Or2Or3Tag => {
            parameter_values(P::Next1Or2Tag, words, i, out)?;
            if i < len - 3 {
                out.push(tag(i + 3));
            }
        }
        P::PrevTag => {
            if i > 0 {
                out.push(tag(i - 1));
            }
        }
        P::Prev1Or2Tag => {
            if i > 0 {
                out.push(tag(i - 1));
            }
            if i > 1 {
                out.push(tag(i - 2));
            }
        }
        P::Prev1Or2Or3Tag => {
            parameter_values(P::Prev1Or2Tag, words, i, out)?;
            if i > 2 {
                out.push(tag(i - 3));
            }
        }
        P::TwoTagAfter => {
            if i < len - 2 {
                out.push(tag(i + 2));
            }
        }
        P::TwoTagBefore => {
            if i > 1 {
                out.push(tag(i - 2));
            }
        }
        P::NextWord => {
            if i < len - 1 {
                out.push(token(i + 1));
            }
        }
        P::PrevWord => {
            if i > 0 {
                out.push(token(i - 1));
            }
        }
        P::TwoWordAfter => {
            if i < len - 2 {
                out.push(token(i + 2));
            }
        }
        P::TwoWordBefore => {
            if i > 1 {
                out.push(token(i - 2));
            }
        }
        P::Next1Or2Word => {
            if i < len - 1 {
                out.push(token(i + 1));
            }
            if i < len - 2 {
                out.push(token(i + 2));
            }
        }
        P::EndsWithConstant => out.push(Some(Cow::Borrowed("ing"))),
        P::CurrentWordBroken => return Err(TaggerError::undefined("token")),
        P::Prev1Or2WordBroken => {
            // `if (i > 0) values.push(sentence[i - 1].token)` — the guard passes,
            // then the read throws. At i == 0 nothing is read and `[]` is
            // returned, so the crash depends on the site.
            if i > 0 {
                return Err(TaggerError::undefined("token"));
            }
        }
    }
    Ok(())
}

/// `RuleTemplate.windowFitsSite`: whether both window edges land in the sentence.
#[must_use]
pub fn window_fits_site(window: [i32; 2], sentence_len: usize, i: isize) -> bool {
    let len = sentence_len as isize;
    let lo = i + isize::try_from(window[0]).expect("window offsets are within -2..=2");
    let hi = i + isize::try_from(window[1]).expect("window offsets are within -2..=2");
    lo >= 0 && lo < len && hi >= 0 && hi < len
}

/// One entry of a trainer's template list: a predicate name bound to its
/// [`Template`] metadata.
///
/// `new RuleTemplate(name, ruleTemplates[name])` in the reference. The two
/// arguments are independent there — nothing stops a caller pairing a name with
/// another entry's metadata — but every use in the library and its spec passes
/// the matching pair, so [`RuleTemplate::named`] is the only constructor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RuleTemplate {
    /// The name rules generated from this template will carry.
    pub predicate_name: &'static str,
    /// The metadata: window, arity and parameter sources.
    pub meta: &'static Template,
}

impl RuleTemplate {
    /// Looks the template up by name, or `None` if there is no such entry.
    ///
    /// The reference has no such guard: `new RuleTemplate('nope', undefined)`
    /// constructs happily and only fails later, inside `windowFitsSite`, with
    /// `Cannot read properties of undefined (reading 'window')`. Nothing in the
    /// exported surface can reach that state, so it is not modelled.
    #[must_use]
    pub fn named(name: &str) -> Option<Self> {
        let meta = template(name)?;
        Some(Self {
            predicate_name: meta.name,
            meta,
        })
    }

    /// Whether both window edges land inside `sentence` at position `i`.
    #[must_use]
    pub fn window_fits_site(&self, sentence: &crate::sentence::Sentence<'_>, i: isize) -> bool {
        window_fits_site(self.meta.window, sentence.len(), i)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sent(pairs: &[(&'static str, Option<&'static str>)]) -> Vec<TaggedWord<'static>> {
        pairs
            .iter()
            .map(|(t, g)| TaggedWord::new(*t, g.map(Cow::Borrowed)))
            .collect()
    }

    fn ev(kind: PredicateKind, w: &[TaggedWord<'_>], i: isize, p1: Option<&str>) -> bool {
        evaluate(kind, w, i, p1, None).unwrap().is_truthy()
    }

    #[test]
    fn table_has_thirty_six_entries_in_order() {
        assert_eq!(TEMPLATES.len(), 36);
        assert_eq!(TEMPLATES[0].name, "NEXT-TAG");
        assert_eq!(TEMPLATES[35].name, "DEFAULT");
    }

    #[test]
    fn ends_with_is_first_occurrence_not_suffix() {
        // The single highest-impact divergence: `str::ends_with` disagrees on
        // every word containing the suffix more than once.
        let w = sent(&[("cats", None)]);
        assert!(ev(PredicateKind::CurrentWordEndsWith, &w, 0, Some("s")));
        let w = sent(&[("sees", None)]);
        assert!(!ev(PredicateKind::CurrentWordEndsWith, &w, 0, Some("s")));
        let w = sent(&[("ss", None)]);
        assert!(!ev(PredicateKind::CurrentWordEndsWith, &w, 0, Some("s")));
        let w = sent(&[("s", None)]);
        assert!(ev(PredicateKind::CurrentWordEndsWith, &w, 0, Some("s")));
        let w = sent(&[("singing", None)]);
        assert!(!ev(PredicateKind::CurrentWordEndsWith, &w, 0, Some("ing")));
        let w = sent(&[("string", None)]);
        assert!(ev(PredicateKind::CurrentWordEndsWith, &w, 0, Some("ing")));
        let w = sent(&[("lly", None)]);
        assert!(ev(PredicateKind::CurrentWordEndsWith, &w, 0, Some("ly")));
        // Empty and absent parameters are both falsy.
        let w = sent(&[("cats", None)]);
        assert!(!ev(PredicateKind::CurrentWordEndsWith, &w, 0, Some("")));
        assert!(!ev(PredicateKind::CurrentWordEndsWith, &w, 0, None));
    }

    #[test]
    fn ends_with_counts_utf16_units() {
        let w = sent(&[("😀s", None)]);
        assert!(ev(PredicateKind::CurrentWordEndsWith, &w, 0, Some("s")));
        let w = sent(&[("cafés", None)]);
        assert!(ev(PredicateKind::CurrentWordEndsWith, &w, 0, Some("és")));
    }

    #[test]
    fn number_predicate_returns_numbers() {
        let w = sent(&[("5abc", None)]);
        let v = evaluate(PredicateKind::CurrentWordIsNumber, &w, 0, Some("YES"), None).unwrap();
        assert_eq!(v, PredValue::Num(5.0));
        assert!(v.is_truthy());

        let w = sent(&[("0abc", None)]);
        let v = evaluate(PredicateKind::CurrentWordIsNumber, &w, 0, Some("YES"), None).unwrap();
        assert_eq!(v, PredValue::Num(0.0));
        assert!(!v.is_truthy(), "parseFloat('0abc') is 0, which is falsy");
        // ...and `NO` therefore fires where `YES` did not.
        let v = evaluate(PredicateKind::CurrentWordIsNumber, &w, 0, Some("NO"), None).unwrap();
        assert!(v.is_truthy());

        // The empty string IS a number to `Number()`.
        let w = sent(&[("", None)]);
        assert!(ev(PredicateKind::CurrentWordIsNumber, &w, 0, Some("YES")));
    }

    #[test]
    fn next_word_is_returns_undefined_at_the_end() {
        let w = sent(&[("a", None), ("b", None)]);
        assert_eq!(
            evaluate(PredicateKind::NextWordIs, &w, 1, Some("z"), None).unwrap(),
            PredValue::Undefined
        );
        assert_eq!(
            evaluate(PredicateKind::NextWordIs, &w, 0, Some("b"), None).unwrap(),
            PredValue::Bool(true)
        );
    }

    #[test]
    fn null_sentinels_never_match_undefined() {
        // prev1Or2Tag seeds its slots with `null`, so at position 0 a rule whose
        // parameter is `undefined` must NOT match, even though `undefined` is
        // what an out-of-range tag would read as.
        let w = sent(&[("a", None), ("b", None)]);
        assert!(!ev(PredicateKind::Prev1Or2Tag, &w, 0, None));
        // ...but an in-range word with no tag does match.
        assert!(ev(PredicateKind::Prev1Or2Tag, &w, 1, None));
    }

    #[test]
    fn wdand2tagaft_is_dead() {
        let w = sent(&[("er", Some("X")), ("b", Some("Y")), ("c", Some("Z"))]);
        // With a defined parameter 2 it can never fire, however well the
        // sentence matches.
        assert!(
            !evaluate(
                PredicateKind::CurrentWordAnd2TagAfterAre,
                &w,
                0,
                Some("er"),
                Some("Z")
            )
            .unwrap()
            .is_truthy()
        );
        // Only a one-parameter rule degenerates into "token === parameter1".
        assert!(
            evaluate(
                PredicateKind::CurrentWordAnd2TagAfterAre,
                &w,
                0,
                Some("er"),
                None
            )
            .unwrap()
            .is_truthy()
        );
    }

    #[test]
    fn empty_token_throws_in_the_cap_predicates() {
        let w = sent(&[("", None), ("x", None)]);
        for k in [
            PredicateKind::CurrentWordIsCap,
            PredicateKind::NextWordIsCap,
            PredicateKind::PrevWordIsCap,
        ] {
            let i = if k == PredicateKind::NextWordIsCap {
                -1
            } else {
                0
            };
            let i = if k == PredicateKind::PrevWordIsCap {
                1
            } else {
                i
            };
            assert_eq!(
                evaluate(k, &w, i, None, None),
                Err(TaggerError::undefined("toUpperCase")),
                "{k:?}"
            );
        }
    }

    #[test]
    fn prev_word_is_is_case_insensitive_and_throws_without_a_parameter() {
        let w = sent(&[("The", None), ("x", None)]);
        assert!(ev(PredicateKind::PrevWordIs, &w, 1, Some("tHe")));
        assert_eq!(
            evaluate(PredicateKind::PrevWordIs, &w, 1, None, None),
            Err(TaggerError::undefined("toLowerCase"))
        );
        // Position 0 short-circuits before the parameter is touched.
        assert_eq!(
            evaluate(PredicateKind::PrevWordIs, &w, 0, None, None),
            Ok(PredValue::Bool(false))
        );
    }

    #[test]
    fn unicode_lowercasing_is_not_ascii_only() {
        let w = sent(&[("ΟΔΟΣ", None), ("x", None)]);
        assert!(ev(PredicateKind::PrevWordIs, &w, 1, Some("οδος")));
    }

    #[test]
    fn prototype_named_predicates_throw() {
        let w = sent(&[("a", None)]);
        for name in OBJECT_PROTOTYPE_MEMBERS {
            assert_eq!(resolve(name), PredicateKind::PrototypeLeak, "{name}");
        }
        assert_eq!(
            evaluate(PredicateKind::PrototypeLeak, &w, 0, None, None),
            Err(TaggerError::PredicateNotAFunction)
        );
        assert_eq!(resolve("UNKNOWNPRED"), PredicateKind::Default);
    }

    #[test]
    fn window_bounds() {
        // NEXT2TAG [0, 2] fits at 0 and 1 of a four-word sentence; PREV2TAG
        // [-2, 0] fits at 2 and 3.
        assert!(window_fits_site([0, 2], 4, 0));
        assert!(window_fits_site([0, 2], 4, 1));
        assert!(!window_fits_site([0, 2], 4, 2));
        assert!(!window_fits_site([-2, 0], 4, 1));
        assert!(window_fits_site([-2, 0], 4, 2));
        assert!(!window_fits_site([0, 0], 0, 0));
    }

    #[test]
    fn broken_parameter_sources_throw() {
        let w = sent(&[("a", None), ("b", None)]);
        let mut out = Vec::new();
        assert!(parameter_values(P::CurrentWordBroken, &w, 0, &mut out).is_err());
        // PREV1OR2WD only crashes once there is a previous word to read.
        assert!(parameter_values(P::Prev1Or2WordBroken, &w, 0, &mut out).is_ok());
        assert!(parameter_values(P::Prev1Or2WordBroken, &w, 1, &mut out).is_err());
        out.clear();
        parameter_values(P::EndsWithConstant, &w, 1, &mut out).unwrap();
        assert_eq!(out, vec![Some(Cow::Borrowed("ing"))]);
    }
}
