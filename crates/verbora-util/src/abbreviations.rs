//! Known-abbreviation lists for English and Spanish.

use std::sync::LazyLock;

use crate::data;

/// English abbreviations, in source order.
///
/// 24 entries, all distinct. Order carries no meaning — see
/// [`AbbreviationLanguage`].
pub use crate::data::ABBREVIATIONS_EN;
/// Spanish abbreviations, in source order.
///
/// 108 entries in four thematic groups — titles, general, legal, legal Latin —
/// of which 107 are distinct, and some contain internal spaces (`"et al."`,
/// `"a posteriori."`). Order carries no meaning — see [`AbbreviationLanguage`].
pub use crate::data::ABBREVIATIONS_ES;

/// A language with an abbreviation list.
///
/// # What these lists are for
///
/// They are ready-made argument lists for
/// `verbora_tokenizers::SentenceTokenizer::with_abbreviations`, which is the
/// only thing in Verbora that consumes them. That consumer defines what a useful
/// entry looks like, and its rule is short enough to quote:
///
/// > A boundary position `b` with `0 < b < text.len()` is **suppressed** if some
/// > abbreviation `a` in the set satisfies
/// > `text[..b].trim_end_matches(char::is_whitespace).ends_with(a)`.
///
/// Four consequences follow, and each is pinned by an enumerating test below:
///
/// * **Matching is case-sensitive** and compares scalar sequences exactly.
///   Nothing folds case, here or in the tokenizer, so `"dr."` is not `"Dr."`. A
///   caller who wants both casings supplies both strings.
/// * **Order is not load-bearing.** The tokenizer asks whether *any*
///   abbreviation matches, not which one, so permuting a set cannot change a
///   single boundary. `"e.g."` and `"i.e."` are independent of each other in
///   either order.
/// * **Duplicates are inert**, for the same reason. [`ABBREVIATIONS_ES`]
///   contains `"loc. cit."` twice; the second copy changes nothing but the
///   length of the slice.
/// * **Matching is by suffix, so a short entry shadows a longer one that ends
///   with it.** With both `"cit."` and `"loc. cit."` in the set, the longer is
///   never the reason a boundary is suppressed. Seven Spanish entries are
///   shadowed this way and `every_abbreviation_is_usable` names them.
///
/// One asymmetry is invisible from the list itself: an entry that does not end
/// in a sentence terminator can only ever suppress a *paragraph* break, since
/// those are the only boundaries whose preceding text does not end in `.`, `!`
/// or `?`. English `"c/o"` and Spanish `"n.os"` are the two such entries.
///
/// # The text unit
///
/// Exactly as for [`crate::Language`]: entries and queries are compared as
/// Unicode scalar sequences, with no case folding, no normalisation and no
/// trimming. Every entry is NFC, enumerated by `every_entry_is_already_nfc`.
///
/// # Choosing the right API
///
/// | Want | Call | Cost |
/// |---|---|---|
/// | Is this exact string an entry? | [`AbbreviationLanguage::contains`] | O(log n) binary search |
/// | The whole list, chosen at runtime | [`AbbreviationLanguage::abbreviations`] | free, borrowed, one `match` |
/// | The whole list, known at compile time | [`ABBREVIATIONS_EN`] / [`ABBREVIATIONS_ES`] | free, and usable in a `const` |
///
/// The last two return the same slice. Name the static when the language is
/// fixed in the source — it needs no value to dispatch on and can appear in
/// const context; call the method when the language is a value you were handed.
///
/// Note that none of the three answers the tokenizer's question, which is about
/// *suffixes*: `contains("casino.")` is `false` even though the entry `"no."`
/// would suppress a boundary after it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[non_exhaustive]
pub enum AbbreviationLanguage {
    /// English.
    En,
    /// Spanish.
    Es,
}

/// Every language with an abbreviation list.
pub static ABBREVIATION_LANGUAGES: &[AbbreviationLanguage] =
    &[AbbreviationLanguage::En, AbbreviationLanguage::Es];

/// Sorted, de-duplicated views, built once on first use.
///
/// Derived rather than typed out beside the source lists, for the reason
/// `crate::stopwords::SORTED` gives at length.
static SORTED: LazyLock<[Box<[&'static str]>; 2]> = LazyLock::new(|| {
    [AbbreviationLanguage::En, AbbreviationLanguage::Es].map(|language| {
        let mut all = language.abbreviations().to_vec();
        all.sort_unstable();
        all.dedup();
        all.into_boxed_slice()
    })
});

impl AbbreviationLanguage {
    /// The ISO 639-1 code, lower-case.
    pub fn code(self) -> &'static str {
        match self {
            Self::En => "en",
            Self::Es => "es",
        }
    }

    /// The shipped list, in source order, duplicates included.
    ///
    /// Both are observable because this borrows the backing slice, and both are
    /// inert as far as sentence boundaries go.
    pub fn abbreviations(self) -> &'static [&'static str] {
        match self {
            Self::En => data::ABBREVIATIONS_EN,
            Self::Es => data::ABBREVIATIONS_ES,
        }
    }

    /// Whether `text` is exactly an entry on this list.
    ///
    /// A membership test over the list, not the tokenizer's rule — see the
    /// type-level note. Compares scalar sequences exactly; `""` is never an
    /// entry. O(log n) by binary search over a de-duplicated view built on first
    /// use.
    ///
    /// ```
    /// use verbora_util::AbbreviationLanguage;
    ///
    /// assert!(AbbreviationLanguage::En.contains("Dr."));
    /// assert!(!AbbreviationLanguage::En.contains("dr."));
    /// assert!(!AbbreviationLanguage::En.contains("casino."));
    /// ```
    pub fn contains(self, text: &str) -> bool {
        let index = match self {
            Self::En => 0,
            Self::Es => 1,
        };
        SORTED[index].binary_search(&text).is_ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use unicode_normalization::UnicodeNormalization;

    /// `SentenceTokenizer`'s suppression rule, restated.
    ///
    /// `verbora-util` sits below `verbora-tokenizers` and does not depend on it,
    /// so these tests cannot call the real tokenizer. This is the predicate
    /// quoted in the type-level note, transcribed from its published contract
    /// rather than from its implementation, and it is used only to ask whether an
    /// *entry* is shaped like something that rule could ever match.
    fn suppresses(abbreviations: &[&str], text_before_boundary: &str) -> bool {
        let head = text_before_boundary.trim_end_matches(char::is_whitespace);
        abbreviations
            .iter()
            .any(|abbreviation| head.ends_with(abbreviation))
    }

    #[test]
    fn list_shapes() {
        assert_eq!(ABBREVIATION_LANGUAGES.len(), 2);
        let en = AbbreviationLanguage::En.abbreviations();
        assert_eq!(en.len(), 24);
        assert_eq!(en[0], "approx.");
        assert_eq!(en[en.len() - 1], "i.e.");
        // The static and the method are the same slice.
        assert!(std::ptr::eq(en, ABBREVIATIONS_EN));

        let es = AbbreviationLanguage::Es.abbreviations();
        assert_eq!(es.len(), 108);
        assert!(std::ptr::eq(es, ABBREVIATIONS_ES));
        assert_eq!(es.iter().filter(|a| **a == "loc. cit.").count(), 2);
        assert!(es.contains(&"et al."));
        assert!(es.contains(&"a posteriori."));
        assert!(es.iter().any(|a| a.contains('í')));

        for lang in ABBREVIATION_LANGUAGES {
            assert_eq!(AbbreviationLanguage::En.code(), "en");
            assert!(!lang.abbreviations().is_empty());
        }
        assert_eq!(AbbreviationLanguage::Es.code(), "es");
    }

    #[test]
    fn membership_compares_scalar_sequences_exactly() {
        assert!(AbbreviationLanguage::En.contains("Dr."));
        assert!(!AbbreviationLanguage::En.contains("dr."));
        assert!(!AbbreviationLanguage::En.contains("DR."));
        assert!(!AbbreviationLanguage::En.contains("Dr"));
        assert!(!AbbreviationLanguage::En.contains("Dr. "));
        assert!(AbbreviationLanguage::Es.contains("Sr."));
        for lang in ABBREVIATION_LANGUAGES {
            assert!(
                !lang.contains(""),
                "{} matched the empty string",
                lang.code()
            );
        }
    }

    /// The derived view is exactly `sort ∘ dedup` of the source list.
    #[test]
    fn the_sorted_view_is_derived_from_the_source_list() {
        for &lang in ABBREVIATION_LANGUAGES {
            let mut expected = lang.abbreviations().to_vec();
            expected.sort_unstable();
            expected.dedup();
            for entry in lang.abbreviations() {
                assert!(lang.contains(entry), "{} lost {entry:?}", lang.code());
            }
            let found = expected.iter().filter(|e| lang.contains(e)).count();
            assert_eq!(found, expected.len());
        }
    }

    /// Entries are NFC, enumerated. Spanish carries accented entries, so this is
    /// not vacuous.
    #[test]
    fn every_entry_is_already_nfc() {
        let mut accented = 0;
        for &lang in ABBREVIATION_LANGUAGES {
            for &entry in lang.abbreviations() {
                let nfc: String = entry.nfc().collect();
                assert_eq!(nfc, entry, "{}: {entry:?} is not NFC", lang.code());
                if !entry.is_ascii() {
                    accented += 1;
                }
            }
        }
        assert!(accented > 0, "nothing non-ASCII was actually checked");
    }

    /// Every entry of both lists, walked through the rule that consumes it.
    ///
    /// The same enumeration discipline the stop-word lists get, for the same
    /// reason: an entry no rule can match is dead weight that looks exactly like
    /// coverage. Two things are checked for every entry and two sets are pinned:
    ///
    /// * the entry can match **on its own** — non-empty, and carrying no
    ///   trailing whitespace. Trailing whitespace is fatal and has no exemption:
    ///   the rule trims the head before comparing, so a head can never end in
    ///   whitespace and `ends_with` can never succeed. This is the
    ///   abbreviation-side twin of the Dutch `"je "` stop word;
    /// * the entry can matter **in company** — some position exists where it
    ///   suppresses and no other entry would have. Entries that fail this are
    ///   shadowed by a shorter suffix of themselves and are pinned by name;
    /// * the entries that cannot suppress a sentence-terminator boundary at all,
    ///   because they do not end in one, are pinned by name too.
    #[test]
    fn every_abbreviation_is_usable() {
        for (lang, shadowed, no_terminator) in [
            (AbbreviationLanguage::En, &[][..], &["c/o"][..]),
            (
                AbbreviationLanguage::Es,
                &[
                    "C.P.C.C.",
                    "Profs.",
                    "c/ap.",
                    "cap.",
                    "ibid.",
                    "loc. cit.",
                    "op. cit.",
                ][..],
                &["n.os"][..],
            ),
        ] {
            let all = lang.abbreviations();
            let mut unmatchable = Vec::new();
            let mut found_shadowed = Vec::new();
            let mut found_no_terminator = Vec::new();

            for &entry in all {
                assert!(!entry.is_empty(), "{}: an entry is empty", lang.code());

                // On its own, at a boundary it should govern.
                let head = format!("Some text {entry}");
                if !suppresses(&[entry], &head) {
                    unmatchable.push(entry);
                    continue;
                }

                // In company: is any *other* entry already a suffix of this one?
                // If so this entry never decides anything.
                let others: Vec<&str> = all
                    .iter()
                    .copied()
                    .filter(|other| *other != entry)
                    .collect();
                if suppresses(&others, &head) && !found_shadowed.contains(&entry) {
                    found_shadowed.push(entry);
                }

                if !entry.ends_with(['.', '!', '?']) && !found_no_terminator.contains(&entry) {
                    found_no_terminator.push(entry);
                }
            }

            assert!(
                unmatchable.is_empty(),
                "{}: {} entries can never match — the rule trims the head before \
                 comparing, so an entry ending in whitespace is unreachable: \
                 {unmatchable:?}",
                lang.code(),
                unmatchable.len()
            );
            found_shadowed.sort_unstable();
            assert_eq!(
                found_shadowed,
                shadowed,
                "{}: the set of entries shadowed by a shorter suffix changed",
                lang.code()
            );
            assert_eq!(
                found_no_terminator,
                no_terminator,
                "{}: the set of entries that cannot end a sentence changed",
                lang.code()
            );
        }
    }

    /// Order and repetition cannot change a suppression decision.
    ///
    /// This replaces a claim these lists used to carry — that the tokenizer built
    /// a leftmost-first regex alternation from them, so `"e.g."` had to precede
    /// `"i.e."`. There is no regex and no alternation: the rule asks whether
    /// *any* entry is a suffix of the trimmed head, and `any` is insensitive to
    /// both order and repetition. Checked over every entry of both lists rather
    /// than argued.
    #[test]
    fn order_and_duplicates_do_not_change_a_decision() {
        for &lang in ABBREVIATION_LANGUAGES {
            let forward: Vec<&str> = lang.abbreviations().to_vec();
            let mut reversed = forward.clone();
            reversed.reverse();
            let mut doubled = forward.clone();
            doubled.extend_from_slice(&forward);

            for &entry in lang.abbreviations() {
                for head in [format!("Some text {entry}"), format!("{entry}   ")] {
                    let want = suppresses(&forward, &head);
                    assert!(want, "{}: {entry:?} suppresses nothing", lang.code());
                    assert_eq!(suppresses(&reversed, &head), want);
                    assert_eq!(suppresses(&doubled, &head), want);
                }
            }
            // A head that no entry ends: still no suppression, in any order.
            assert!(!suppresses(&forward, "plain words"));
            assert!(!suppresses(&reversed, "plain words"));
        }
    }

    /// The rule is a suffix test, not an equality test, and it over-suppresses.
    ///
    /// Recorded here because it is the one property of these lists a caller must
    /// understand before adopting them wholesale: `"no."` is on the English
    /// list, so a sentence ending in `"casino."` will not break.
    #[test]
    fn suffix_matching_over_suppresses_and_that_is_the_contract() {
        let en: Vec<&str> = AbbreviationLanguage::En.abbreviations().to_vec();
        assert!(en.contains(&"no."));
        assert!(suppresses(&en, "Visit the casino."));
        assert!(!AbbreviationLanguage::En.contains("casino."));
    }
}
