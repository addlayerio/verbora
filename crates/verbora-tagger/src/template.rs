//! Rule templates. The public surface is [`Template`] and its three sets, all
//! re-exported from the crate root; this module is private, so its own header
//! would not reach the published documentation.

use crate::condition::Condition;
use crate::tag::{Tag, TaggedToken, Word};
use crate::text;

/// A rule shape [`Trainer`](crate::Trainer) instantiates at a training site.
///
/// A template is a [`Condition`] with its arguments left open. At a site, a
/// template proposes every condition of its shape that actually *holds* there —
/// proposing one that does not hold would be proposing a rule that cannot fire
/// at the site it was generated for.
///
/// There is exactly one template per [`Condition`] variant, and
/// [`Template::instantiate`] is the only place that maps between them, so no
/// condition exists that no template can propose, and no template proposes a
/// condition that does not exist.
///
/// See [`Condition`] for what each shape means and which paper it comes from.
/// Three ready-made sets are provided: [`Template::CONTEXTUAL`],
/// [`Template::LEXICALIZED`] and [`Template::ALL`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
#[allow(missing_docs)] // each variant is documented on the matching `Condition`.
pub enum Template {
    PrevTag,
    NextTag,
    PrevTag2,
    NextTag2,
    PrevTagWithin2,
    NextTagWithin2,
    PrevTagWithin3,
    NextTagWithin3,
    SurroundingTags,
    PrevTagBigram,
    NextTagBigram,
    CurrentWord,
    PrevWord,
    NextWord,
    PrevWordWithin2,
    NextWordWithin2,
    LeftWordBigram,
    RightWordBigram,
    CurrentWordAndPrevTag,
    CurrentWordAndNextTag,
    CurrentWordAndTag2Before,
    CurrentWordAndTag2After,
    CurrentWordAndWord2After,
    CurrentWordIsCapitalized,
    NextWordIsCapitalized,
    PrevWordIsCapitalized,
    CurrentWordIsNumeral,
    CurrentWordLooksLikeUrl,
    CurrentWordEndsWith,
}

const CONTEXTUAL: &[Template] = &[
    Template::PrevTag,
    Template::NextTag,
    Template::PrevTag2,
    Template::NextTag2,
    Template::PrevTagWithin2,
    Template::NextTagWithin2,
    Template::PrevTagWithin3,
    Template::NextTagWithin3,
    Template::SurroundingTags,
    Template::PrevTagBigram,
    Template::NextTagBigram,
];

const LEXICALIZED: &[Template] = &[
    Template::CurrentWord,
    Template::PrevWord,
    Template::NextWord,
    Template::PrevWordWithin2,
    Template::NextWordWithin2,
    Template::LeftWordBigram,
    Template::RightWordBigram,
    Template::CurrentWordAndPrevTag,
    Template::CurrentWordAndNextTag,
    Template::CurrentWordAndTag2Before,
    Template::CurrentWordAndTag2After,
    Template::CurrentWordAndWord2After,
    Template::CurrentWordIsCapitalized,
    Template::NextWordIsCapitalized,
    Template::PrevWordIsCapitalized,
    Template::CurrentWordIsNumeral,
    Template::CurrentWordLooksLikeUrl,
    Template::CurrentWordEndsWith,
];

const ALL: &[Template] = &[
    Template::PrevTag,
    Template::NextTag,
    Template::PrevTag2,
    Template::NextTag2,
    Template::PrevTagWithin2,
    Template::NextTagWithin2,
    Template::PrevTagWithin3,
    Template::NextTagWithin3,
    Template::SurroundingTags,
    Template::PrevTagBigram,
    Template::NextTagBigram,
    Template::CurrentWord,
    Template::PrevWord,
    Template::NextWord,
    Template::PrevWordWithin2,
    Template::NextWordWithin2,
    Template::LeftWordBigram,
    Template::RightWordBigram,
    Template::CurrentWordAndPrevTag,
    Template::CurrentWordAndNextTag,
    Template::CurrentWordAndTag2Before,
    Template::CurrentWordAndTag2After,
    Template::CurrentWordAndWord2After,
    Template::CurrentWordIsCapitalized,
    Template::NextWordIsCapitalized,
    Template::PrevWordIsCapitalized,
    Template::CurrentWordIsNumeral,
    Template::CurrentWordLooksLikeUrl,
    Template::CurrentWordEndsWith,
];

impl Template {
    /// How many trailing scalars a [`Template::CurrentWordEndsWith`] suffix may
    /// have.
    ///
    /// Brill (1995) §3 draws unknown-word suffix rules from suffixes of up to
    /// four characters; this is that bound, in Unicode scalar values.
    pub const MAX_SUFFIX_SCALARS: usize = 4;

    /// The contextual templates of Brill (1992), which test **tags only**.
    ///
    /// This is the set to train with when the corpus is small: lexicalised
    /// templates instantiate against individual tokens, and with little data
    /// they learn rules that memorise the training set.
    pub const CONTEXTUAL: &'static [Template] = CONTEXTUAL;

    /// The lexicalised templates of Brill (1994), plus Verbora's three
    /// token-shape templates.
    pub const LEXICALIZED: &'static [Template] = LEXICALIZED;

    /// Every template: [`Template::CONTEXTUAL`] followed by
    /// [`Template::LEXICALIZED`]. This is [`Trainer`](crate::Trainer)'s default.
    pub const ALL: &'static [Template] = ALL;
}

/// The word at `i + delta`, or `None` when it falls outside `words` or is not a
/// conforming [`Word`].
fn word_at(words: &[TaggedToken<'_>], i: usize, delta: isize) -> Option<Word> {
    let j = i.checked_add_signed(delta)?;
    let token = words.get(j)?.token();
    Word::new(token.to_owned()).ok()
}

fn tag_at<'w>(words: &'w [TaggedToken<'_>], i: usize, delta: isize) -> Option<&'w Tag> {
    let j = i.checked_add_signed(delta)?;
    Some(&words.get(j)?.tag)
}

impl Template {
    /// Appends every condition of this shape that holds at position `i`.
    ///
    /// Nothing is appended when the template's window falls outside the
    /// sentence, or when a token it would name is not a conforming [`Word`] —
    /// a token containing whitespace can never be written into a rule string, so
    /// a rule naming it could never be round-tripped.
    ///
    /// Every appended condition satisfies `condition.holds(words, i)`;
    /// `tests::instantiations_hold_where_they_were_generated` enumerates that
    /// over every template at every position of a mixed sentence.
    pub fn instantiate(self, words: &[TaggedToken<'_>], i: usize, out: &mut Vec<Condition>) {
        if i >= words.len() {
            // There is no site here, so there is nothing to propose — even for a
            // template whose window would land inside the sentence.
            return;
        }
        let t = |d: isize| tag_at(words, i, d).cloned();
        let w = |d: isize| word_at(words, i, d);
        // Where the caller starts writing, so the within-window templates can
        // deduplicate only what this call added.
        let first = out.len();
        match self {
            Self::PrevTag => out.extend(t(-1).map(Condition::PrevTag)),
            Self::NextTag => out.extend(t(1).map(Condition::NextTag)),
            Self::PrevTag2 => out.extend(t(-2).map(Condition::PrevTag2)),
            Self::NextTag2 => out.extend(t(2).map(Condition::NextTag2)),
            Self::PrevTagWithin2 => {
                out.extend(t(-1).map(Condition::PrevTagWithin2));
                out.extend(t(-2).map(Condition::PrevTagWithin2));
            }
            Self::NextTagWithin2 => {
                out.extend(t(1).map(Condition::NextTagWithin2));
                out.extend(t(2).map(Condition::NextTagWithin2));
            }
            Self::PrevTagWithin3 => {
                for d in [-1, -2, -3] {
                    out.extend(t(d).map(Condition::PrevTagWithin3));
                }
            }
            Self::NextTagWithin3 => {
                for d in [1, 2, 3] {
                    out.extend(t(d).map(Condition::NextTagWithin3));
                }
            }
            Self::SurroundingTags => {
                if let (Some(prev), Some(next)) = (t(-1), t(1)) {
                    out.push(Condition::SurroundingTags { prev, next });
                }
            }
            Self::PrevTagBigram => {
                if let (Some(two_before), Some(before)) = (t(-2), t(-1)) {
                    out.push(Condition::PrevTagBigram { two_before, before });
                }
            }
            Self::NextTagBigram => {
                if let (Some(after), Some(two_after)) = (t(1), t(2)) {
                    out.push(Condition::NextTagBigram { after, two_after });
                }
            }
            Self::CurrentWord => out.extend(w(0).map(Condition::CurrentWord)),
            Self::PrevWord => out.extend(w(-1).map(Condition::PrevWord)),
            Self::NextWord => out.extend(w(1).map(Condition::NextWord)),
            Self::PrevWordWithin2 => {
                out.extend(w(-1).map(Condition::PrevWordWithin2));
                out.extend(w(-2).map(Condition::PrevWordWithin2));
            }
            Self::NextWordWithin2 => {
                out.extend(w(1).map(Condition::NextWordWithin2));
                out.extend(w(2).map(Condition::NextWordWithin2));
            }
            Self::LeftWordBigram => {
                if let (Some(before), Some(current)) = (w(-1), w(0)) {
                    out.push(Condition::LeftWordBigram { before, current });
                }
            }
            Self::RightWordBigram => {
                if let (Some(current), Some(after)) = (w(0), w(1)) {
                    out.push(Condition::RightWordBigram { current, after });
                }
            }
            Self::CurrentWordAndPrevTag => {
                if let (Some(prev_tag), Some(word)) = (t(-1), w(0)) {
                    out.push(Condition::CurrentWordAndPrevTag { prev_tag, word });
                }
            }
            Self::CurrentWordAndNextTag => {
                if let (Some(word), Some(next_tag)) = (w(0), t(1)) {
                    out.push(Condition::CurrentWordAndNextTag { word, next_tag });
                }
            }
            Self::CurrentWordAndTag2Before => {
                if let (Some(tag_two_before), Some(word)) = (t(-2), w(0)) {
                    out.push(Condition::CurrentWordAndTag2Before {
                        tag_two_before,
                        word,
                    });
                }
            }
            Self::CurrentWordAndTag2After => {
                if let (Some(word), Some(tag_two_after)) = (w(0), t(2)) {
                    out.push(Condition::CurrentWordAndTag2After {
                        word,
                        tag_two_after,
                    });
                }
            }
            Self::CurrentWordAndWord2After => {
                if let (Some(word), Some(word_two_after)) = (w(0), w(2)) {
                    out.push(Condition::CurrentWordAndWord2After {
                        word,
                        word_two_after,
                    });
                }
            }
            Self::CurrentWordIsCapitalized => {
                if let Some(x) = words.get(i) {
                    out.push(Condition::CurrentWordIsCapitalized(text::is_capitalized(
                        x.token(),
                    )));
                }
            }
            Self::NextWordIsCapitalized => {
                if let Some(x) = i.checked_add(1).and_then(|j| words.get(j)) {
                    out.push(Condition::NextWordIsCapitalized(text::is_capitalized(
                        x.token(),
                    )));
                }
            }
            Self::PrevWordIsCapitalized => {
                if let Some(x) = i.checked_sub(1).and_then(|j| words.get(j)) {
                    out.push(Condition::PrevWordIsCapitalized(text::is_capitalized(
                        x.token(),
                    )));
                }
            }
            Self::CurrentWordIsNumeral => {
                if let Some(x) = words.get(i) {
                    out.push(Condition::CurrentWordIsNumeral(text::is_numeral(x.token())));
                }
            }
            Self::CurrentWordLooksLikeUrl => {
                if let Some(x) = words.get(i) {
                    out.push(Condition::CurrentWordLooksLikeUrl(text::looks_like_url(
                        x.token(),
                    )));
                }
            }
            Self::CurrentWordEndsWith => {
                let Some(token) = words.get(i).map(TaggedToken::token) else {
                    return;
                };
                // Suffixes of 1..=MAX_SUFFIX_SCALARS scalars, longest first.
                let mut boundaries: Vec<usize> = token
                    .char_indices()
                    .rev()
                    .take(Self::MAX_SUFFIX_SCALARS)
                    .map(|(at, _)| at)
                    .collect();
                boundaries.reverse();
                for at in boundaries {
                    out.extend(
                        Word::new(token[at..].to_owned())
                            .ok()
                            .map(Condition::CurrentWordEndsWith),
                    );
                }
            }
        }

        // A window template asks one question — "is the tag at −1 *or* −2 a
        // DT?" — so it must contribute one condition, not one per position it
        // looked at. Emitting a duplicate makes the trainer credit the same
        // token twice: a rule fixing exactly one site scored 2, clearing the
        // `min_score` threshold that exists precisely to stop a rule from
        // memorising a single site, and `TrainingStep::corrections` — "tokens
        // it changed from wrong to right" — reported two for one token.
        // `PrevTagWithin3`/`NextTagWithin3` could triple-count.
        if out.len() - first > 1 {
            let mut seen = first;
            while seen < out.len() {
                if out[first..seen].contains(&out[seen]) {
                    out.remove(seen);
                } else {
                    seen += 1;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    /// A window template contributes one condition per site, not one per
    /// position it inspected.
    ///
    /// `PREV-1-OR-2-TAG` looks at −1 and −2. When both carry the same tag the
    /// two lookups produce the *same* `Condition`, and emitting it twice makes
    /// the trainer credit one corrected token twice. The corpus below has
    /// exactly one mis-tagged token, so no rule can honestly score 2 — yet
    /// before this deduplication one did, clearing the `min_score` threshold
    /// that exists to stop a rule from memorising a single site.
    #[test]
    fn a_window_template_credits_a_repeated_tag_once() {
        let words = [tok("the", "DT"), tok("the", "DT"), tok("dog", "VB")];
        let mut out = Vec::new();
        Template::PrevTagWithin2.instantiate(&words, 2, &mut out);
        assert_eq!(
            out,
            vec![Condition::PrevTagWithin2(
                Tag::new("DT").expect("valid tag")
            )]
        );
    }

    /// Distinct tags in the window still contribute one condition each.
    #[test]
    fn a_window_template_keeps_distinct_tags() {
        let words = [tok("a", "DT"), tok("big", "JJ"), tok("dog", "VB")];
        let mut out = Vec::new();
        Template::PrevTagWithin2.instantiate(&words, 2, &mut out);
        assert_eq!(out.len(), 2);
    }

    use super::*;

    fn tok(t: &'static str, g: &'static str) -> TaggedToken<'static> {
        TaggedToken::new(t, Tag::new(g).unwrap())
    }

    fn sentence() -> Vec<TaggedToken<'static>> {
        vec![
            tok("The", "DT"),
            tok("2,500", "CD"),
            tok("www.a.com", "URL"),
            tok("running", "VBG"),
            tok("dogs", "NNS"),
            tok("café", "NN"),
            tok("😀", "NN"),
        ]
    }

    /// Whatever a template proposes at a site must hold at that site — otherwise
    /// the trainer is generating rules that cannot fire where they were found.
    /// Enumerated over every template at every position, including the ends.
    #[test]
    fn instantiations_hold_where_they_were_generated() {
        let s = sentence();
        let mut out = Vec::new();
        let mut total = 0;
        for template in ALL {
            for i in 0..s.len() {
                out.clear();
                template.instantiate(&s, i, &mut out);
                for c in &out {
                    assert!(
                        c.holds(&s, i),
                        "{template:?} proposed {c} which is false at {i}"
                    );
                    total += 1;
                }
            }
        }
        assert!(total > 100, "the sweep proposed {total} conditions");
    }

    /// Out-of-range sites and an empty sentence propose nothing and never panic.
    #[test]
    fn out_of_range_sites_propose_nothing() {
        let s = sentence();
        let mut out = Vec::new();
        for template in ALL {
            template.instantiate(&[], 0, &mut out);
            template.instantiate(&s, s.len(), &mut out);
            template.instantiate(&s, usize::MAX, &mut out);
        }
        assert!(out.is_empty());
    }

    #[test]
    fn suffix_template_proposes_up_to_four_scalars_longest_first() {
        let s = vec![tok("running", "VBG")];
        let mut out = Vec::new();
        Template::CurrentWordEndsWith.instantiate(&s, 0, &mut out);
        let got: Vec<String> = out
            .iter()
            .map(|c| match c {
                Condition::CurrentWordEndsWith(w) => w.to_string(),
                other => panic!("{other}"),
            })
            .collect();
        assert_eq!(got, ["ning", "ing", "ng", "g"]);

        // Scalars, not bytes: a four-scalar suffix of an astral token is four
        // scalars long, and a two-scalar token proposes only two suffixes.
        let s = vec![tok("a😀", "NN")];
        out.clear();
        Template::CurrentWordEndsWith.instantiate(&s, 0, &mut out);
        let got: Vec<String> = out
            .iter()
            .map(|c| match c {
                Condition::CurrentWordEndsWith(w) => w.to_string(),
                other => panic!("{other}"),
            })
            .collect();
        assert_eq!(got, ["a😀", "😀"]);
    }

    /// A token that cannot be a `Word` yields no lexicalised proposals, rather
    /// than a rule that could not be written out.
    #[test]
    fn non_conforming_tokens_propose_no_lexicalised_conditions() {
        let s = vec![tok("a b", "NN"), tok("c", "NN")];
        let mut out = Vec::new();
        Template::CurrentWord.instantiate(&s, 0, &mut out);
        Template::PrevWord.instantiate(&s, 1, &mut out);
        assert!(out.is_empty());
        // The tag templates are unaffected.
        Template::PrevTag.instantiate(&s, 1, &mut out);
        assert_eq!(out.len(), 1);
    }

    /// `ALL` is exactly `CONTEXTUAL` followed by `LEXICALIZED`, with no
    /// duplicates and nothing missing.
    #[test]
    fn template_lists_agree() {
        let joined: Vec<Template> = CONTEXTUAL.iter().chain(LEXICALIZED).copied().collect();
        assert_eq!(joined, ALL);
        let mut sorted: Vec<String> = ALL.iter().map(|t| format!("{t:?}")).collect();
        sorted.sort();
        sorted.dedup();
        assert_eq!(sorted.len(), ALL.len(), "duplicate template");
    }

    /// One template per `Condition` variant.
    #[test]
    fn every_condition_variant_has_a_template() {
        assert_eq!(ALL.len(), 29);
    }
}
