//! The condition a transformation rule tests before it fires.
//!
//! # Where the set comes from
//!
//! The templates are Brill's, from the two papers that define transformation-based
//! part-of-speech tagging:
//!
//! * Eric Brill, *A Simple Rule-Based Part of Speech Tagger*, ANLP-92, pages
//!   152–155 — the six **non-lexicalised** contextual templates: the preceding
//!   (following) word is tagged *z*; the word two before (after) is tagged *z*;
//!   one of the two (three) preceding (following) words is tagged *z*; the
//!   preceding word is tagged *z* and the following word is tagged *w*; the
//!   preceding (following) word is tagged *z* and the word two before (after) is
//!   tagged *w*.
//! * Eric Brill, *Some Advances in Transformation-Based Part of Speech Tagging*,
//!   AAAI-94 — the **lexicalised** templates, which test tokens instead of, or
//!   alongside, tags.
//!
//! Three conditions are Verbora's own rather than Brill's, because the bundled
//! English rule set uses them: [`Condition::CurrentWordIsNumeral`],
//! [`Condition::CurrentWordLooksLikeUrl`] and
//! [`Condition::CurrentWordEndsWith`]. Each carries its own full definition on
//! the variant itself, because a rule written against one has to keep meaning
//! the same thing across versions.
//!
//! # Every condition is total
//!
//! A condition reads at most three positions either side of the site. Positions
//! outside the sentence simply do not match; there is no error case, no panic,
//! and no distinction between "absent" and "present but different". Evaluation
//! is therefore infallible, which is what lets [`crate::BrillTagger::tag`]
//! return a value rather than a `Result`.

use std::fmt;

use crate::tag::{Tag, TaggedToken, Word};
use crate::text;

/// The condition under which a [`Rule`](crate::Rule) rewrites a tag.
///
/// Each variant carries exactly the arguments it uses: a condition with a
/// missing or surplus argument is not representable, and neither is one whose
/// argument is a tag where a token was meant.
///
/// # Where the set comes from
///
/// The templates are Brill's, from the two papers that define
/// transformation-based part-of-speech tagging:
///
/// * Eric Brill, *A Simple Rule-Based Part of Speech Tagger*, ANLP-92,
///   152–155 — the **non-lexicalised** contextual templates: the preceding
///   (following) word is tagged *z*; the word two before (after) is tagged *z*;
///   one of the two (three) preceding (following) words is tagged *z*; the
///   preceding word is tagged *z* and the following word is tagged *w*; the two
///   preceding (following) tags are *z* and *w*.
/// * Eric Brill, *Some Advances in Transformation-Based Part of Speech
///   Tagging*, AAAI-94 — the **lexicalised** templates, which test tokens
///   instead of, or alongside, tags.
///
/// Three conditions are Verbora's own, because the bundled English rule set
/// uses them: [`Condition::CurrentWordIsNumeral`],
/// [`Condition::CurrentWordLooksLikeUrl`] and
/// [`Condition::CurrentWordEndsWith`]. Each defines itself in full on its own
/// variant.
///
/// # Every condition is total
///
/// A condition reads at most three positions either side of the site. Positions
/// outside the sentence simply do not match; there is no error case, no panic,
/// and no distinction between "absent" and "present but different". Evaluation
/// is infallible, which is what lets [`BrillTagger::tag`](crate::BrillTagger::tag)
/// return a value rather than a `Result`.
///
/// # Rule-string spelling
///
/// Every variant has one canonical name, plus the aliases the bundled data uses.
/// [`Display`](fmt::Display) always writes the canonical name;
/// [`FromStr`](std::str::FromStr) on [`Rule`](crate::Rule) accepts both.
/// `i` below is the site the rule is being tested at.
///
/// | Canonical name | Aliases | Arguments | True when |
/// |---|---|---|---|
/// | `PREV-TAG` | `PREVTAG` | tag | `tag(i-1) == z` |
/// | `NEXT-TAG` | `NEXTTAG`, `NEXT-WORD-IS-TAG` | tag | `tag(i+1) == z` |
/// | `PREV-2-TAG` | `PREV2TAG` | tag | `tag(i-2) == z` |
/// | `NEXT-2-TAG` | `NEXT2TAG` | tag | `tag(i+2) == z` |
/// | `PREV-1-OR-2-TAG` | `PREV1OR2TAG` | tag | `z` in `tag(i-1..=i-2)` |
/// | `NEXT-1-OR-2-TAG` | `NEXT1OR2TAG` | tag | `z` in `tag(i+1..=i+2)` |
/// | `PREV-1-OR-2-OR-3-TAG` | `PREV1OR2OR3TAG` | tag | `z` in `tag(i-1..=i-3)` |
/// | `NEXT-1-OR-2-OR-3-TAG` | `NEXT1OR2OR3TAG` | tag | `z` in `tag(i+1..=i+3)` |
/// | `SURROUND-TAG` | `SURROUNDTAG` | tag, tag | `tag(i-1) == z && tag(i+1) == w` |
/// | `PREV-TAG-BIGRAM` | `PREVBIGRAM` | tag, tag | `tag(i-2) == z && tag(i-1) == w` |
/// | `NEXT-TAG-BIGRAM` | `NEXTBIGRAM` | tag, tag | `tag(i+1) == z && tag(i+2) == w` |
/// | `CURRENT-WORD-IS` | `CURWD` | word | `token(i) == w` |
/// | `PREV-WORD-IS` | `PREVWD` | word | `token(i-1) == w` |
/// | `NEXT-WORD-IS` | `NEXTWD` | word | `token(i+1) == w` |
/// | `PREV-1-OR-2-WORD-IS` | `PREV1OR2WD` | word | `w` in `token(i-1..=i-2)` |
/// | `NEXT-1-OR-2-WORD-IS` | `NEXT1OR2WD` | word | `w` in `token(i+1..=i+2)` |
/// | `LEFT-WORD-BIGRAM` | `LBIGRAM` | word, word | `token(i-1) == w1 && token(i) == w2` |
/// | `RIGHT-WORD-BIGRAM` | `RBIGRAM` | word, word | `token(i) == w1 && token(i+1) == w2` |
/// | `WORD-AND-PREV-TAG` | `WDPREVTAG` | tag, word | `tag(i-1) == z && token(i) == w` |
/// | `WORD-AND-NEXT-TAG` | `WDNEXTTAG` | word, tag | `token(i) == w && tag(i+1) == z` |
/// | `WORD-AND-2-TAG-BEFORE` | `WDAND2TAGBFR` | tag, word | `tag(i-2) == z && token(i) == w` |
/// | `WORD-AND-2-TAG-AFTER` | `WDAND2TAGAFT` | word, tag | `token(i) == w && tag(i+2) == z` |
/// | `WORD-AND-2-WORD-AFTER` | `WDAND2AFT` | word, word | `token(i) == w1 && token(i+2) == w2` |
/// | `CURRENT-WORD-IS-CAP` | — | `YES`/`NO` | `is_capitalized(token(i))` matches |
/// | `NEXT-WORD-IS-CAP` | — | `YES`/`NO` | `is_capitalized(token(i+1))` matches |
/// | `PREV-WORD-IS-CAP` | — | `YES`/`NO` | `is_capitalized(token(i-1))` matches |
/// | `CURRENT-WORD-IS-NUMBER` | — | `YES`/`NO` | `is_numeral(token(i))` matches |
/// | `CURRENT-WORD-IS-URL` | — | `YES`/`NO` | `looks_like_url(token(i))` matches |
/// | `CURRENT-WORD-ENDS-WITH` | — | word | `token(i).ends_with(w)` |
///
/// Note the argument order of the four mixed word/tag conditions: two of them
/// take the tag first and two take the word first. That is the order the bundled
/// Dutch rules are written in, and it is preserved rather than normalised
/// because normalising it would silently invert 21 shipped rules. The field
/// names make it unambiguous in Rust.
///
/// # Comparison is exact
///
/// Word comparisons are byte-exact. Nothing is case-folded, trimmed or
/// normalised on either side: `PREV-WORD-IS would` does not match `Would`. Case
/// insensitivity, when it is wanted, is the caller's explicit choice — fold the
/// tokens before tagging, or write both rules.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Condition {
    /// The preceding word is tagged `z`. Brill (1992), template 1.
    PrevTag(Tag),
    /// The following word is tagged `z`. Brill (1992), template 1.
    NextTag(Tag),
    /// The word two before is tagged `z`. Brill (1992), template 2.
    PrevTag2(Tag),
    /// The word two after is tagged `z`. Brill (1992), template 2.
    NextTag2(Tag),
    /// One of the two preceding words is tagged `z`. Brill (1992), template 3.
    PrevTagWithin2(Tag),
    /// One of the two following words is tagged `z`. Brill (1992), template 3.
    NextTagWithin2(Tag),
    /// One of the three preceding words is tagged `z`. Brill (1992), template 4.
    PrevTagWithin3(Tag),
    /// One of the three following words is tagged `z`. Brill (1992), template 4.
    NextTagWithin3(Tag),
    /// The preceding word is tagged `prev` and the following word is tagged
    /// `next`. Brill (1992), template 5.
    SurroundingTags {
        /// The tag required at `i-1`.
        prev: Tag,
        /// The tag required at `i+1`.
        next: Tag,
    },
    /// The two preceding tags are `two_before` then `before`. Brill (1992),
    /// template 6.
    PrevTagBigram {
        /// The tag required at `i-2`.
        two_before: Tag,
        /// The tag required at `i-1`.
        before: Tag,
    },
    /// The two following tags are `after` then `two_after`. Brill (1992),
    /// template 6.
    NextTagBigram {
        /// The tag required at `i+1`.
        after: Tag,
        /// The tag required at `i+2`.
        two_after: Tag,
    },
    /// The current word is `w`. Brill (1994), lexicalised.
    CurrentWord(Word),
    /// The preceding word is `w`. Brill (1994), lexicalised.
    PrevWord(Word),
    /// The following word is `w`. Brill (1994), lexicalised.
    NextWord(Word),
    /// One of the two preceding words is `w`. Brill (1994), lexicalised.
    PrevWordWithin2(Word),
    /// One of the two following words is `w`. Brill (1994), lexicalised.
    NextWordWithin2(Word),
    /// The preceding word is `before` and the current word is `current`.
    LeftWordBigram {
        /// The token required at `i-1`.
        before: Word,
        /// The token required at `i`.
        current: Word,
    },
    /// The current word is `current` and the following word is `after`.
    RightWordBigram {
        /// The token required at `i`.
        current: Word,
        /// The token required at `i+1`.
        after: Word,
    },
    /// The current word is `word` and the preceding word is tagged `prev_tag`.
    CurrentWordAndPrevTag {
        /// The tag required at `i-1`. Written **first** in a rule string.
        prev_tag: Tag,
        /// The token required at `i`.
        word: Word,
    },
    /// The current word is `word` and the following word is tagged `next_tag`.
    CurrentWordAndNextTag {
        /// The token required at `i`. Written **first** in a rule string.
        word: Word,
        /// The tag required at `i+1`.
        next_tag: Tag,
    },
    /// The current word is `word` and the word two before is tagged
    /// `tag_two_before`.
    CurrentWordAndTag2Before {
        /// The tag required at `i-2`. Written **first** in a rule string.
        tag_two_before: Tag,
        /// The token required at `i`.
        word: Word,
    },
    /// The current word is `word` and the word two after is tagged
    /// `tag_two_after`.
    CurrentWordAndTag2After {
        /// The token required at `i`. Written **first** in a rule string.
        word: Word,
        /// The tag required at `i+2`.
        tag_two_after: Tag,
    },
    /// The current word is `word` and the word two after is `word_two_after`.
    CurrentWordAndWord2After {
        /// The token required at `i`.
        word: Word,
        /// The token required at `i+2`.
        word_two_after: Word,
    },
    /// The current word's capitalisation is as given. Brill (1992), Table 1.
    ///
    /// # What "capitalised" means
    ///
    /// The token's **first Unicode scalar has the derived `Uppercase` property**
    /// ([UAX #44]: `Lu` plus `Other_Uppercase`). The same definition decides
    /// which default [`Lexicon::tag_of`](crate::Lexicon::tag_of) applies to an
    /// unknown token, so the two can never disagree.
    ///
    /// | Token | Capitalised | Why |
    /// |---|---|---|
    /// | `"Dog"` | yes | `D` is `Lu` |
    /// | `"Ålesund"`, `"Москва"`, `"Ελλάς"` | yes | non-ASCII `Lu`; this is **not** an `A`–`Z` test |
    /// | `"İstanbul"` | yes | U+0130 is `Lu` |
    /// | `"𝐀bc"` | yes | U+1D400 MATHEMATICAL BOLD CAPITAL A is `Lu`, read whole rather than as a surrogate |
    /// | `"ǅungla"` | no | U+01C5 is `Lt` (titlecase), which is not `Uppercase` |
    /// | `"5"`, `"."`, `"日本"`, `"😀"` | no | no case at all |
    /// | `""` | no | there is no first scalar |
    ///
    /// [UAX #44]: https://www.unicode.org/reports/tr44/#Uppercase
    CurrentWordIsCapitalized(bool),
    /// The following word's capitalisation is as given. See
    /// [`Condition::CurrentWordIsCapitalized`] for what capitalisation means.
    NextWordIsCapitalized(bool),
    /// The preceding word's capitalisation is as given. See
    /// [`Condition::CurrentWordIsCapitalized`] for what capitalisation means.
    PrevWordIsCapitalized(bool),
    /// Whether the current word is a Verbora **numeral**.
    ///
    /// Verbora's own condition, not Brill's, and the grammar is defined here
    /// rather than delegated to a language runtime's string-to-number coercion,
    /// so the answer does not depend on anything outside this crate:
    ///
    /// ```text
    /// numeral  = sign? mantissa exponent?
    /// sign     = "+" | "-"
    /// mantissa = grouped ( "." digits? )?  |  "." digits
    /// grouped  = digits | digit{1,3} ( "," digit{3} )+
    /// digits   = digit+
    /// digit    = "0".."9"                    (ASCII only, U+0030..U+0039)
    /// exponent = ("e" | "E") sign? digits
    /// ```
    ///
    /// The whole token must match.
    ///
    /// | Token | Numeral | Note |
    /// |---|---|---|
    /// | `"5"`, `"-2"`, `"+5"`, `"3.14"`, `".5"`, `"5."` | yes | |
    /// | `"1e5"`, `"1E-5"` | yes | complete exponent |
    /// | `"1,000"`, `"12,345,678"` | yes | groups of exactly three after the first |
    /// | `""` | **no** | an empty token is not a numeral |
    /// | `"1,0000"`, `"10,00"`, `",100"` | no | malformed grouping |
    /// | `"5abc"`, `"1_000"`, `"1e"`, `"."`, `"-"` | no | trailing or incomplete text |
    /// | `"0x10"`, `"Infinity"`, `"NaN"` | no | not decimal literals |
    /// | `"٣"`, `"１２３"` | no | ASCII digits only, by the grammar above |
    ///
    /// The rows most likely to differ from a runtime coercion are `""` (which
    /// several languages call zero), `"5abc"` (which a prefix parser accepts),
    /// `"0x10"` and `"Infinity"`.
    CurrentWordIsNumeral(bool),
    /// Whether the current word *looks like* a URL.
    ///
    /// Verbora's own condition, and a deliberately small heuristic: the token
    /// must contain **some** `U+002E FULL STOP` that is neither its first nor
    /// its last scalar, **and** two consecutive ASCII letters somewhere. That is
    /// the whole rule. It is not an implementation of [RFC 3986]; it exists so
    /// that the bundled `NN URL CURRENT-WORD-IS-URL YES` rule has a definition
    /// to point at.
    ///
    /// The dot test is existential, so a URL that arrives with the sentence's
    /// full stop attached — `www.example.com.`, what whitespace tokenization
    /// produces most often — still qualifies.
    ///
    /// | Token | URL-like | Why |
    /// |---|---|---|
    /// | `"www.example.com"`, `"example.org"` | yes | interior dot, `ww`/`ex` |
    /// | `"www.example.com."`, `".www.example.com"` | yes | the dot at `www.` is still interior |
    /// | `"Ph.D."` | yes | an accepted false positive: interior dot, and `Ph` |
    /// | `"3.14"` | no | no two adjacent ASCII letters |
    /// | `"e.g."`, `"A.A.U."`, `"U.S."` | no | no two adjacent letters |
    /// | `".com"`, `"com."`, `"etc."` | no | the only dot is first or last |
    /// | `"日本.語"` | no | the letters are not ASCII |
    ///
    /// [RFC 3986]: https://www.rfc-editor.org/rfc/rfc3986
    CurrentWordLooksLikeUrl(bool),
    /// The current word ends with the given suffix.
    ///
    /// A genuine suffix test — `str::ends_with`, which for well-formed UTF-8 is
    /// a scalar-sequence suffix match. `"sees"` **does** end with `"s"`, and
    /// `"singing"` **does** end with `"ing"`.
    CurrentWordEndsWith(Word),
}

/// One position of `words` relative to `i`, or `None` when it falls outside.
#[inline]
fn at<'w, 'a>(words: &'w [TaggedToken<'a>], i: usize, delta: isize) -> Option<&'w TaggedToken<'a>> {
    let j = i.checked_add_signed(delta)?;
    words.get(j)
}

#[inline]
fn tag_is(words: &[TaggedToken<'_>], i: usize, delta: isize, want: &Tag) -> bool {
    at(words, i, delta).is_some_and(|w| &w.tag == want)
}

#[inline]
fn token_is(words: &[TaggedToken<'_>], i: usize, delta: isize, want: &Word) -> bool {
    at(words, i, delta).is_some_and(|w| &*w.token == want.as_str())
}

#[inline]
fn cap_is(words: &[TaggedToken<'_>], i: usize, delta: isize, want: bool) -> bool {
    at(words, i, delta).is_some_and(|w| text::is_capitalized(&w.token) == want)
}

impl Condition {
    /// Whether the condition holds at position `i` of `words`.
    ///
    /// Positions outside `words` never match, and `i` itself may be out of
    /// range: the answer is then `false` for every variant that reads position
    /// `i`, and unchanged for the rest. There is no panic and no error.
    #[must_use]
    pub fn holds(&self, words: &[TaggedToken<'_>], i: usize) -> bool {
        match self {
            Self::PrevTag(z) => tag_is(words, i, -1, z),
            Self::NextTag(z) => tag_is(words, i, 1, z),
            Self::PrevTag2(z) => tag_is(words, i, -2, z),
            Self::NextTag2(z) => tag_is(words, i, 2, z),
            Self::PrevTagWithin2(z) => tag_is(words, i, -1, z) || tag_is(words, i, -2, z),
            Self::NextTagWithin2(z) => tag_is(words, i, 1, z) || tag_is(words, i, 2, z),
            Self::PrevTagWithin3(z) => {
                tag_is(words, i, -1, z) || tag_is(words, i, -2, z) || tag_is(words, i, -3, z)
            }
            Self::NextTagWithin3(z) => {
                tag_is(words, i, 1, z) || tag_is(words, i, 2, z) || tag_is(words, i, 3, z)
            }
            Self::SurroundingTags { prev, next } => {
                tag_is(words, i, -1, prev) && tag_is(words, i, 1, next)
            }
            Self::PrevTagBigram { two_before, before } => {
                tag_is(words, i, -2, two_before) && tag_is(words, i, -1, before)
            }
            Self::NextTagBigram { after, two_after } => {
                tag_is(words, i, 1, after) && tag_is(words, i, 2, two_after)
            }
            Self::CurrentWord(w) => token_is(words, i, 0, w),
            Self::PrevWord(w) => token_is(words, i, -1, w),
            Self::NextWord(w) => token_is(words, i, 1, w),
            Self::PrevWordWithin2(w) => token_is(words, i, -1, w) || token_is(words, i, -2, w),
            Self::NextWordWithin2(w) => token_is(words, i, 1, w) || token_is(words, i, 2, w),
            Self::LeftWordBigram { before, current } => {
                token_is(words, i, -1, before) && token_is(words, i, 0, current)
            }
            Self::RightWordBigram { current, after } => {
                token_is(words, i, 0, current) && token_is(words, i, 1, after)
            }
            Self::CurrentWordAndPrevTag { prev_tag, word } => {
                tag_is(words, i, -1, prev_tag) && token_is(words, i, 0, word)
            }
            Self::CurrentWordAndNextTag { word, next_tag } => {
                token_is(words, i, 0, word) && tag_is(words, i, 1, next_tag)
            }
            Self::CurrentWordAndTag2Before {
                tag_two_before,
                word,
            } => tag_is(words, i, -2, tag_two_before) && token_is(words, i, 0, word),
            Self::CurrentWordAndTag2After {
                word,
                tag_two_after,
            } => token_is(words, i, 0, word) && tag_is(words, i, 2, tag_two_after),
            Self::CurrentWordAndWord2After {
                word,
                word_two_after,
            } => token_is(words, i, 0, word) && token_is(words, i, 2, word_two_after),
            Self::CurrentWordIsCapitalized(want) => cap_is(words, i, 0, *want),
            Self::NextWordIsCapitalized(want) => cap_is(words, i, 1, *want),
            Self::PrevWordIsCapitalized(want) => cap_is(words, i, -1, *want),
            Self::CurrentWordIsNumeral(want) => {
                at(words, i, 0).is_some_and(|w| text::is_numeral(&w.token) == *want)
            }
            Self::CurrentWordLooksLikeUrl(want) => {
                at(words, i, 0).is_some_and(|w| text::looks_like_url(&w.token) == *want)
            }
            Self::CurrentWordEndsWith(suffix) => {
                at(words, i, 0).is_some_and(|w| w.token.ends_with(suffix.as_str()))
            }
        }
    }

    /// How far the condition reads, as `(left, right)` positions from the site.
    ///
    /// Never more than three in either direction — that is the widest template
    /// Brill (1992) defines, and it is what bounds
    /// [`BrillTagger::tag_stream`](crate::BrillTagger::tag_stream)'s buffer.
    ///
    /// Unlike a hand-maintained window table, this is derived from the variant
    /// itself, so a condition cannot declare a window narrower than what it
    /// actually reads.
    #[must_use]
    pub const fn reach(&self) -> (usize, usize) {
        match self {
            Self::CurrentWord(_)
            | Self::CurrentWordIsCapitalized(_)
            | Self::CurrentWordIsNumeral(_)
            | Self::CurrentWordLooksLikeUrl(_)
            | Self::CurrentWordEndsWith(_) => (0, 0),
            Self::PrevTag(_)
            | Self::PrevWord(_)
            | Self::PrevWordIsCapitalized(_)
            | Self::LeftWordBigram { .. }
            | Self::CurrentWordAndPrevTag { .. } => (1, 0),
            Self::NextTag(_)
            | Self::NextWord(_)
            | Self::NextWordIsCapitalized(_)
            | Self::RightWordBigram { .. }
            | Self::CurrentWordAndNextTag { .. } => (0, 1),
            Self::PrevTag2(_)
            | Self::PrevTagWithin2(_)
            | Self::PrevWordWithin2(_)
            | Self::PrevTagBigram { .. }
            | Self::CurrentWordAndTag2Before { .. } => (2, 0),
            Self::NextTag2(_)
            | Self::NextTagWithin2(_)
            | Self::NextWordWithin2(_)
            | Self::NextTagBigram { .. }
            | Self::CurrentWordAndTag2After { .. }
            | Self::CurrentWordAndWord2After { .. } => (0, 2),
            Self::SurroundingTags { .. } => (1, 1),
            Self::PrevTagWithin3(_) => (3, 0),
            Self::NextTagWithin3(_) => (0, 3),
        }
    }

    /// The canonical name written by [`Display`](fmt::Display).
    #[must_use]
    pub const fn name(&self) -> &'static str {
        match self {
            Self::PrevTag(_) => "PREV-TAG",
            Self::NextTag(_) => "NEXT-TAG",
            Self::PrevTag2(_) => "PREV-2-TAG",
            Self::NextTag2(_) => "NEXT-2-TAG",
            Self::PrevTagWithin2(_) => "PREV-1-OR-2-TAG",
            Self::NextTagWithin2(_) => "NEXT-1-OR-2-TAG",
            Self::PrevTagWithin3(_) => "PREV-1-OR-2-OR-3-TAG",
            Self::NextTagWithin3(_) => "NEXT-1-OR-2-OR-3-TAG",
            Self::SurroundingTags { .. } => "SURROUND-TAG",
            Self::PrevTagBigram { .. } => "PREV-TAG-BIGRAM",
            Self::NextTagBigram { .. } => "NEXT-TAG-BIGRAM",
            Self::CurrentWord(_) => "CURRENT-WORD-IS",
            Self::PrevWord(_) => "PREV-WORD-IS",
            Self::NextWord(_) => "NEXT-WORD-IS",
            Self::PrevWordWithin2(_) => "PREV-1-OR-2-WORD-IS",
            Self::NextWordWithin2(_) => "NEXT-1-OR-2-WORD-IS",
            Self::LeftWordBigram { .. } => "LEFT-WORD-BIGRAM",
            Self::RightWordBigram { .. } => "RIGHT-WORD-BIGRAM",
            Self::CurrentWordAndPrevTag { .. } => "WORD-AND-PREV-TAG",
            Self::CurrentWordAndNextTag { .. } => "WORD-AND-NEXT-TAG",
            Self::CurrentWordAndTag2Before { .. } => "WORD-AND-2-TAG-BEFORE",
            Self::CurrentWordAndTag2After { .. } => "WORD-AND-2-TAG-AFTER",
            Self::CurrentWordAndWord2After { .. } => "WORD-AND-2-WORD-AFTER",
            Self::CurrentWordIsCapitalized(_) => "CURRENT-WORD-IS-CAP",
            Self::NextWordIsCapitalized(_) => "NEXT-WORD-IS-CAP",
            Self::PrevWordIsCapitalized(_) => "PREV-WORD-IS-CAP",
            Self::CurrentWordIsNumeral(_) => "CURRENT-WORD-IS-NUMBER",
            Self::CurrentWordLooksLikeUrl(_) => "CURRENT-WORD-IS-URL",
            Self::CurrentWordEndsWith(_) => "CURRENT-WORD-ENDS-WITH",
        }
    }
}

#[cfg(test)]
impl Condition {
    /// The tag arguments this condition compares a neighbour's tag against.
    ///
    /// Word arguments are deliberately excluded: a word argument is compared
    /// against caller-supplied token text, which no bundled data constrains, so
    /// it says nothing about whether a rule can fire. A tag argument does — a
    /// tag no configuration can produce makes the condition false everywhere.
    ///
    /// Written as an exhaustive match so that a new variant has to declare its
    /// arguments here rather than defaulting to "none".
    pub(crate) const fn tag_arguments(&self) -> [Option<&Tag>; 2] {
        match self {
            Self::PrevTag(t)
            | Self::NextTag(t)
            | Self::PrevTag2(t)
            | Self::NextTag2(t)
            | Self::PrevTagWithin2(t)
            | Self::NextTagWithin2(t)
            | Self::PrevTagWithin3(t)
            | Self::NextTagWithin3(t)
            | Self::CurrentWordAndPrevTag { prev_tag: t, .. }
            | Self::CurrentWordAndNextTag { next_tag: t, .. }
            | Self::CurrentWordAndTag2Before {
                tag_two_before: t, ..
            }
            | Self::CurrentWordAndTag2After {
                tag_two_after: t, ..
            } => [Some(t), None],
            Self::SurroundingTags { prev: a, next: b }
            | Self::PrevTagBigram {
                two_before: a,
                before: b,
            }
            | Self::NextTagBigram {
                after: a,
                two_after: b,
            } => [Some(a), Some(b)],
            Self::CurrentWord(_)
            | Self::PrevWord(_)
            | Self::NextWord(_)
            | Self::PrevWordWithin2(_)
            | Self::NextWordWithin2(_)
            | Self::LeftWordBigram { .. }
            | Self::RightWordBigram { .. }
            | Self::CurrentWordAndWord2After { .. }
            | Self::CurrentWordIsCapitalized(_)
            | Self::NextWordIsCapitalized(_)
            | Self::PrevWordIsCapitalized(_)
            | Self::CurrentWordIsNumeral(_)
            | Self::CurrentWordLooksLikeUrl(_)
            | Self::CurrentWordEndsWith(_) => [None, None],
        }
    }
}

/// `YES` / `NO`, the rule-string spelling of a boolean argument.
pub(crate) const fn yes_no(v: bool) -> &'static str {
    if v { "YES" } else { "NO" }
}

impl fmt::Display for Condition {
    /// `NAME arg…`, the tail of a rule string.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())?;
        match self {
            Self::PrevTag(a)
            | Self::NextTag(a)
            | Self::PrevTag2(a)
            | Self::NextTag2(a)
            | Self::PrevTagWithin2(a)
            | Self::NextTagWithin2(a)
            | Self::PrevTagWithin3(a)
            | Self::NextTagWithin3(a) => write!(f, " {a}"),
            Self::CurrentWord(a)
            | Self::PrevWord(a)
            | Self::NextWord(a)
            | Self::PrevWordWithin2(a)
            | Self::NextWordWithin2(a)
            | Self::CurrentWordEndsWith(a) => write!(f, " {a}"),
            Self::SurroundingTags { prev: a, next: b }
            | Self::PrevTagBigram {
                two_before: a,
                before: b,
            }
            | Self::NextTagBigram {
                after: a,
                two_after: b,
            } => write!(f, " {a} {b}"),
            Self::LeftWordBigram {
                before: a,
                current: b,
            }
            | Self::RightWordBigram {
                current: a,
                after: b,
            }
            | Self::CurrentWordAndWord2After {
                word: a,
                word_two_after: b,
            } => write!(f, " {a} {b}"),
            Self::CurrentWordAndPrevTag {
                prev_tag: a,
                word: b,
            }
            | Self::CurrentWordAndTag2Before {
                tag_two_before: a,
                word: b,
            } => write!(f, " {a} {b}"),
            Self::CurrentWordAndNextTag {
                word: a,
                next_tag: b,
            }
            | Self::CurrentWordAndTag2After {
                word: a,
                tag_two_after: b,
            } => write!(f, " {a} {b}"),
            Self::CurrentWordIsCapitalized(v)
            | Self::NextWordIsCapitalized(v)
            | Self::PrevWordIsCapitalized(v)
            | Self::CurrentWordIsNumeral(v)
            | Self::CurrentWordLooksLikeUrl(v) => write!(f, " {}", yes_no(*v)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tok(t: &'static str, g: &'static str) -> TaggedToken<'static> {
        TaggedToken::new(t, Tag::new(g).unwrap())
    }

    fn tag(s: &'static str) -> Tag {
        Tag::new(s).unwrap()
    }

    fn word(s: &'static str) -> Word {
        Word::new(s).unwrap()
    }

    fn sentence() -> Vec<TaggedToken<'static>> {
        vec![
            tok("the", "DT"),
            tok("quick", "JJ"),
            tok("brown", "JJ"),
            tok("fox", "NN"),
            tok("Runs", "VBZ"),
        ]
    }

    #[test]
    fn tag_conditions() {
        let s = sentence();
        assert!(Condition::PrevTag(tag("JJ")).holds(&s, 3));
        assert!(!Condition::PrevTag(tag("DT")).holds(&s, 3));
        assert!(Condition::PrevTag2(tag("JJ")).holds(&s, 3));
        assert!(Condition::NextTag(tag("VBZ")).holds(&s, 3));
        assert!(Condition::NextTag2(tag("VBZ")).holds(&s, 2));
        assert!(Condition::PrevTagWithin2(tag("JJ")).holds(&s, 3));
        assert!(Condition::PrevTagWithin3(tag("DT")).holds(&s, 3));
        assert!(!Condition::PrevTagWithin2(tag("DT")).holds(&s, 3));
        assert!(Condition::NextTagWithin3(tag("VBZ")).holds(&s, 1));
        assert!(
            Condition::SurroundingTags {
                prev: tag("JJ"),
                next: tag("VBZ")
            }
            .holds(&s, 3)
        );
        assert!(
            Condition::PrevTagBigram {
                two_before: tag("JJ"),
                before: tag("JJ")
            }
            .holds(&s, 3)
        );
        assert!(
            Condition::NextTagBigram {
                after: tag("NN"),
                two_after: tag("VBZ")
            }
            .holds(&s, 2)
        );
    }

    /// A condition that reaches past either end of the sentence is simply
    /// false — no panic, no error, no difference between "absent" and "other".
    #[test]
    fn out_of_range_never_matches_and_never_panics() {
        let s = sentence();
        for i in 0..s.len() {
            for c in [
                Condition::PrevTag(tag("DT")),
                Condition::NextTag(tag("DT")),
                Condition::PrevTagWithin3(tag("DT")),
                Condition::NextTagWithin3(tag("DT")),
                Condition::PrevWordIsCapitalized(true),
                Condition::NextWordIsCapitalized(true),
            ] {
                let _ = c.holds(&s, i);
            }
        }
        assert!(!Condition::PrevTag(tag("DT")).holds(&s, 0));
        assert!(!Condition::NextTag(tag("DT")).holds(&s, 4));
        // An empty sentence, and an index past the end of a non-empty one.
        assert!(!Condition::CurrentWord(word("the")).holds(&[], 0));
        assert!(!Condition::CurrentWordIsNumeral(true).holds(&s, 99));
        assert!(!Condition::CurrentWordIsCapitalized(false).holds(&s, 99));
    }

    #[test]
    fn word_conditions_are_case_sensitive() {
        let s = sentence();
        assert!(Condition::PrevWord(word("fox")).holds(&s, 4));
        assert!(Condition::CurrentWord(word("Runs")).holds(&s, 4));
        assert!(
            !Condition::CurrentWord(word("runs")).holds(&s, 4),
            "word comparison is exact, never folded"
        );
        assert!(Condition::PrevWordWithin2(word("brown")).holds(&s, 4));
        assert!(Condition::NextWordWithin2(word("Runs")).holds(&s, 2));
        assert!(
            Condition::LeftWordBigram {
                before: word("fox"),
                current: word("Runs")
            }
            .holds(&s, 4)
        );
        assert!(
            Condition::RightWordBigram {
                current: word("fox"),
                after: word("Runs")
            }
            .holds(&s, 3)
        );
        assert!(
            Condition::CurrentWordAndWord2After {
                word: word("brown"),
                word_two_after: word("Runs")
            }
            .holds(&s, 2)
        );
    }

    #[test]
    fn mixed_word_and_tag_conditions_keep_their_argument_order() {
        let s = sentence();
        assert!(
            Condition::CurrentWordAndPrevTag {
                prev_tag: tag("JJ"),
                word: word("fox")
            }
            .holds(&s, 3)
        );
        assert!(
            Condition::CurrentWordAndNextTag {
                word: word("fox"),
                next_tag: tag("VBZ")
            }
            .holds(&s, 3)
        );
        assert!(
            Condition::CurrentWordAndTag2Before {
                tag_two_before: tag("JJ"),
                word: word("fox")
            }
            .holds(&s, 3)
        );
        assert!(
            Condition::CurrentWordAndTag2After {
                word: word("brown"),
                tag_two_after: tag("VBZ")
            }
            .holds(&s, 2)
        );
    }

    /// The suffix test is a real suffix test. Values here are read off
    /// `str::ends_with`, whose meaning is fixed by the standard library, not by
    /// this crate's output.
    #[test]
    fn ends_with_is_a_suffix_test() {
        for (token, suffix, want) in [
            ("cats", "s", true),
            ("sees", "s", true),
            ("ss", "s", true),
            ("s", "s", true),
            ("singing", "ing", true),
            ("string", "ing", true),
            ("lly", "ly", true),
            ("cats", "z", false),
            ("s", "ss", false),
            ("café", "é", true),
            ("😀s", "s", true),
            ("cafés", "és", true),
            ("a😀", "😀", true),
        ] {
            let s = vec![tok(token, "NN")];
            assert_eq!(
                Condition::CurrentWordEndsWith(Word::new(suffix).unwrap()).holds(&s, 0),
                want,
                "{token:?} ends_with {suffix:?}"
            );
        }
    }

    #[test]
    fn boolean_conditions_honour_their_argument() {
        let s = vec![tok("Dog", "NN"), tok("cat", "NN"), tok("5", "CD")];
        assert!(Condition::CurrentWordIsCapitalized(true).holds(&s, 0));
        assert!(!Condition::CurrentWordIsCapitalized(false).holds(&s, 0));
        assert!(Condition::CurrentWordIsCapitalized(false).holds(&s, 1));
        assert!(Condition::PrevWordIsCapitalized(true).holds(&s, 1));
        assert!(Condition::NextWordIsCapitalized(false).holds(&s, 0));
        assert!(Condition::CurrentWordIsNumeral(true).holds(&s, 2));
        assert!(Condition::CurrentWordIsNumeral(false).holds(&s, 1));
        assert!(Condition::CurrentWordLooksLikeUrl(false).holds(&s, 1));
    }

    /// Every variant's declared reach must be at least what `holds` actually
    /// reads. Checked by construction: a condition placed at the centre of a
    /// sentence of `2*R+1` distinct tags must not change its answer when the
    /// sentence is truncated to exactly its declared reach.
    #[test]
    fn declared_reach_covers_what_is_read() {
        const R: usize = 3;
        let all: Vec<TaggedToken<'static>> = vec![
            tok("w0", "T0"),
            tok("w1", "T1"),
            tok("w2", "T2"),
            tok("w3", "T3"),
            tok("w4", "T4"),
            tok("w5", "T5"),
            tok("w6", "T6"),
        ];
        let conditions = every_condition();
        for c in &conditions {
            let (l, r) = c.reach();
            assert!(l <= R && r <= R, "{c} reaches further than three positions");
            let lo = R - l;
            let hi = R + r + 1;
            let trimmed = &all[lo..hi];
            assert_eq!(
                c.holds(&all, R),
                c.holds(trimmed, R - lo),
                "{c} reads outside its declared reach"
            );
        }
    }

    /// One instance of every variant, for the exhaustiveness-flavoured tests.
    /// The `match` below fails to compile if a variant is added without being
    /// listed here.
    pub(super) fn every_condition() -> Vec<Condition> {
        let t = |s: &'static str| Tag::new(s).unwrap();
        let w = |s: &'static str| Word::new(s).unwrap();
        let all = vec![
            Condition::PrevTag(t("T2")),
            Condition::NextTag(t("T4")),
            Condition::PrevTag2(t("T1")),
            Condition::NextTag2(t("T5")),
            Condition::PrevTagWithin2(t("T1")),
            Condition::NextTagWithin2(t("T5")),
            Condition::PrevTagWithin3(t("T0")),
            Condition::NextTagWithin3(t("T6")),
            Condition::SurroundingTags {
                prev: t("T2"),
                next: t("T4"),
            },
            Condition::PrevTagBigram {
                two_before: t("T1"),
                before: t("T2"),
            },
            Condition::NextTagBigram {
                after: t("T4"),
                two_after: t("T5"),
            },
            Condition::CurrentWord(w("w3")),
            Condition::PrevWord(w("w2")),
            Condition::NextWord(w("w4")),
            Condition::PrevWordWithin2(w("w1")),
            Condition::NextWordWithin2(w("w5")),
            Condition::LeftWordBigram {
                before: w("w2"),
                current: w("w3"),
            },
            Condition::RightWordBigram {
                current: w("w3"),
                after: w("w4"),
            },
            Condition::CurrentWordAndPrevTag {
                prev_tag: t("T2"),
                word: w("w3"),
            },
            Condition::CurrentWordAndNextTag {
                word: w("w3"),
                next_tag: t("T4"),
            },
            Condition::CurrentWordAndTag2Before {
                tag_two_before: t("T1"),
                word: w("w3"),
            },
            Condition::CurrentWordAndTag2After {
                word: w("w3"),
                tag_two_after: t("T5"),
            },
            Condition::CurrentWordAndWord2After {
                word: w("w3"),
                word_two_after: w("w5"),
            },
            Condition::CurrentWordIsCapitalized(false),
            Condition::NextWordIsCapitalized(false),
            Condition::PrevWordIsCapitalized(false),
            Condition::CurrentWordIsNumeral(false),
            Condition::CurrentWordLooksLikeUrl(false),
            Condition::CurrentWordEndsWith(w("3")),
        ];
        // Exhaustiveness guard: adding a variant breaks this match.
        for c in &all {
            match c {
                Condition::PrevTag(_)
                | Condition::NextTag(_)
                | Condition::PrevTag2(_)
                | Condition::NextTag2(_)
                | Condition::PrevTagWithin2(_)
                | Condition::NextTagWithin2(_)
                | Condition::PrevTagWithin3(_)
                | Condition::NextTagWithin3(_)
                | Condition::SurroundingTags { .. }
                | Condition::PrevTagBigram { .. }
                | Condition::NextTagBigram { .. }
                | Condition::CurrentWord(_)
                | Condition::PrevWord(_)
                | Condition::NextWord(_)
                | Condition::PrevWordWithin2(_)
                | Condition::NextWordWithin2(_)
                | Condition::LeftWordBigram { .. }
                | Condition::RightWordBigram { .. }
                | Condition::CurrentWordAndPrevTag { .. }
                | Condition::CurrentWordAndNextTag { .. }
                | Condition::CurrentWordAndTag2Before { .. }
                | Condition::CurrentWordAndTag2After { .. }
                | Condition::CurrentWordAndWord2After { .. }
                | Condition::CurrentWordIsCapitalized(_)
                | Condition::NextWordIsCapitalized(_)
                | Condition::PrevWordIsCapitalized(_)
                | Condition::CurrentWordIsNumeral(_)
                | Condition::CurrentWordLooksLikeUrl(_)
                | Condition::CurrentWordEndsWith(_) => {}
            }
        }
        assert_eq!(all.len(), 29, "one instance per variant");
        all
    }

    #[test]
    fn every_variant_has_a_distinct_canonical_name() {
        let names: Vec<&str> = every_condition().iter().map(Condition::name).collect();
        let mut sorted = names.clone();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.len(), names.len(), "duplicate canonical name");
    }
}
