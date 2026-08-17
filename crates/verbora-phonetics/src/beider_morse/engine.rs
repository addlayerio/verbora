//! The rule-application engine: walks an input string left to right,
//! applying the first matching rule at each position, and merges the
//! resulting phoneme candidates — a direct algorithmic port of the
//! `PhonemeBuilder`/`RulesApplication` shape shared by every independent
//! implementation surveyed during design (Apache Commons Codec's Java,
//! `rphonetic`'s Rust), not a reinterpretation.

use super::languages::LanguageSet;
use super::rule::Rule;

/// One candidate phoneme string, still tagged with the set of languages
/// under which it remains a valid reading.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Phoneme {
    pub(super) text: String,
    pub(super) languages: LanguageSet,
}

/// The growing, forking set of candidate phoneme strings for one input,
/// starting from a single empty candidate and branching every time a rule's
/// phonetic output offers more than one alternative.
#[derive(Debug, Clone)]
pub(super) struct PhonemeBuilder {
    /// Deliberately not deduplicating on `languages` — two candidates with
    /// the same text but different surviving language sets are genuinely
    /// different facts ("this spelling is valid under exactly these
    /// languages"), and merging them early would lose that.
    candidates: Vec<Phoneme>,
}

impl PhonemeBuilder {
    pub(super) fn empty(languages: LanguageSet) -> Self {
        Self {
            candidates: vec![Phoneme {
                text: String::new(),
                languages,
            }],
        }
    }

    /// Appends `text` to every candidate unconditionally (used for a
    /// literal pass-through character the rule table has no rule for).
    pub(super) fn append_literal(&mut self, text: &str) {
        for c in &mut self.candidates {
            c.text.push_str(text);
        }
    }

    /// Cross-products the current candidates with a rule's phonetic
    /// alternatives, keeping only combinations whose language sets still
    /// intersect, capped at `max_phonemes` — the same shape as every
    /// reference implementation's `PhonemeBuilder::apply`.
    pub(super) fn apply(
        &mut self,
        phonemes: &[(std::borrow::Cow<'_, str>, LanguageSet)],
        max_phonemes: usize,
    ) {
        let mut next: Vec<Phoneme> = Vec::with_capacity(self.candidates.len());
        'outer: for left in &self.candidates {
            for (text, right_langs) in phonemes {
                let languages = left.languages.intersect(*right_langs);
                if !languages.is_empty() {
                    let mut joined = String::with_capacity(left.text.len() + text.len());
                    joined.push_str(&left.text);
                    joined.push_str(text);
                    next.push(Phoneme {
                        text: joined,
                        languages,
                    });
                    if next.len() >= max_phonemes {
                        break 'outer;
                    }
                }
            }
        }
        self.candidates = next;
    }

    pub(super) fn into_candidates(self) -> Vec<Phoneme> {
        self.candidates
    }
}

/// Rules bucketed by their pattern's first character, in file order within
/// each bucket (matching order is significant: the *first* matching rule in
/// a bucket wins, not the longest or best match).
pub(super) struct RuleTable {
    pub(super) by_first_char: std::collections::HashMap<char, Vec<Rule>>,
}

/// What happens at a position no rule matches. The two passes genuinely
/// differ here (confirmed by reading the reference implementation's own
/// two call sites, not a shared default): the Rules pass's main loop never
/// appends anything for an unmatched position, so it's silently skipped;
/// the Approx/Exact final pass's loop explicitly appends the single
/// character. For ordinary alphabetic input every position always matches
/// some rule (every rules-pass file ends in a catch-all per letter), so
/// this is unobservable there — it only matters for characters no rule
/// covers at all, e.g. the space `concat`-mode fuses between words, which
/// the Rules pass must consume without leaving a literal space in the
/// output.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum OnUnmatched {
    Skip,
    PassThrough,
}

impl RuleTable {
    /// Walks `input` left to right, applying the first matching rule at
    /// each position, building up `builder`'s candidate set as it goes. See
    /// [`OnUnmatched`] for what happens where no rule matches.
    pub(super) fn apply_to(
        &self,
        input: &str,
        builder: &mut PhonemeBuilder,
        max_phonemes: usize,
        on_unmatched: OnUnmatched,
    ) {
        let chars: Vec<(usize, char)> = input.char_indices().collect();
        let mut idx = 0;
        while idx < chars.len() {
            let (byte_pos, ch) = chars[idx];
            let mut matched = false;
            if let Some(rules) = self.by_first_char.get(&ch) {
                for rule in rules {
                    if rule.matches(input, byte_pos) {
                        builder.apply(&rule.phonemes, max_phonemes);
                        // Advance by the pattern's own character count, not
                        // byte length -- ASCII-only pattern alphabet in
                        // every rule file surveyed, but char-counting is the
                        // honest unit here since `idx` is a char index.
                        idx += rule.pattern.chars().count().max(1);
                        matched = true;
                        break;
                    }
                }
            }
            if !matched {
                if on_unmatched == OnUnmatched::PassThrough {
                    let end = chars.get(idx + 1).map_or(input.len(), |&(p, _)| p);
                    builder.append_literal(&input[byte_pos..end]);
                }
                idx += 1;
            }
        }
    }
}

/// Deduplicates a finished candidate list into `text -> merged languages`,
/// merging language sets for identical spellings reached via different
/// branches (mirrors `apply_final_rule`'s own phoneme-merge-by-text step in
/// every reference implementation).
pub(super) fn merge_by_text(candidates: Vec<Phoneme>) -> Vec<Phoneme> {
    let mut merged: std::collections::BTreeMap<String, LanguageSet> =
        std::collections::BTreeMap::new();
    for c in candidates {
        merged
            .entry(c.text)
            .and_modify(|langs| *langs = langs.union(c.languages))
            .or_insert(c.languages);
    }
    merged
        .into_iter()
        .map(|(text, languages)| Phoneme { text, languages })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::beider_morse::languages::Language;

    #[test]
    fn append_literal_appends_to_every_candidate() {
        let mut b = PhonemeBuilder::empty(LanguageSet::all(2));
        b.apply(
            &[
                (std::borrow::Cow::Borrowed("a"), LanguageSet::all(2)),
                (std::borrow::Cow::Borrowed("b"), LanguageSet::all(2)),
            ],
            20,
        );
        b.append_literal("!");
        let texts: Vec<String> = b.into_candidates().into_iter().map(|p| p.text).collect();
        assert_eq!(texts, vec!["a!", "b!"]);
    }

    #[test]
    fn apply_cross_products_and_drops_disjoint_language_sets() {
        let french = LanguageSet::single(Language(0));
        let spanish = LanguageSet::single(Language(1));
        let mut b = PhonemeBuilder::empty(french);
        // One alternative shares a language with the running set (survives,
        // intersected down to just French), one doesn't (dropped entirely).
        b.apply(
            &[
                (std::borrow::Cow::Borrowed("fr"), french),
                (std::borrow::Cow::Borrowed("es"), spanish),
            ],
            20,
        );
        let candidates = b.into_candidates();
        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].text, "fr");
        assert_eq!(candidates[0].languages, french);
    }

    #[test]
    fn apply_stops_at_max_phonemes() {
        let all = LanguageSet::all(1);
        let mut b = PhonemeBuilder::empty(all);
        let alts: Vec<(std::borrow::Cow<'_, str>, LanguageSet)> = (0..10)
            .map(|i| (std::borrow::Cow::Owned(i.to_string()), all))
            .collect();
        b.apply(&alts, 3);
        assert_eq!(b.into_candidates().len(), 3);
    }

    #[test]
    fn merge_by_text_unions_languages_of_identical_spellings_and_sorts() {
        let a = LanguageSet::single(Language(0));
        let b = LanguageSet::single(Language(1));
        let merged = merge_by_text(vec![
            Phoneme {
                text: "z".to_owned(),
                languages: a,
            },
            Phoneme {
                text: "a".to_owned(),
                languages: b,
            },
            Phoneme {
                text: "z".to_owned(),
                languages: b,
            },
        ]);
        // Alphabetical (a `BTreeMap` dedup side effect -- the real,
        // deterministic order `BeiderMorseCode::spellings` documents).
        assert_eq!(merged.len(), 2);
        assert_eq!(merged[0].text, "a");
        assert_eq!(merged[1].text, "z");
        assert_eq!(
            merged[1].languages,
            a.union(b),
            "both branches to \"z\" must be merged, not just one kept"
        );
    }

    #[test]
    fn on_unmatched_skip_drops_the_character_pass_through_keeps_it() {
        let empty_table = RuleTable {
            by_first_char: std::collections::HashMap::new(),
        };
        let languages = LanguageSet::all(1);

        let mut skip_builder = PhonemeBuilder::empty(languages);
        empty_table.apply_to("xyz", &mut skip_builder, 20, OnUnmatched::Skip);
        assert_eq!(skip_builder.into_candidates()[0].text, "");

        let mut pass_builder = PhonemeBuilder::empty(languages);
        empty_table.apply_to("xyz", &mut pass_builder, 20, OnUnmatched::PassThrough);
        assert_eq!(pass_builder.into_candidates()[0].text, "xyz");
    }
}
