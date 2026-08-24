//! Every entry of every rule table, walked through the prelude that guards it.
//!
//! # The failure this exists to catch
//!
//! [`super::audit`] does this for the stop-word lists. A rule table is the same
//! shape of data with the same failure mode, one stage further in: between the
//! token a caller passes and the string a suffix table is finally searched
//! against there is a **prelude** — lowercasing, the German `ß` expansion, the
//! Italian acute-to-grave rewrite and `I`/`U` marking, the French `I`/`U`/`Y`
//! marking, the Dutch accent fold, the Portuguese nasal detour, the Russian `ё`
//! fold — and every one of them can make a table entry *unreachable*: still in
//! the table, still searched for, and never again matched by anything. Nothing
//! fails. The rule simply stops existing.
//!
//! That is the same defect as a stop word the tokenizer can no longer produce,
//! and it had already happened here twice before anyone enumerated:
//!
//! * Spanish [`crate::PorterStemmerEs`] listed **`"  aseis"`** — with two
//!   leading spaces — where the `-aseis` imperfect-subjunctive ending belongs.
//!   No token can contain a space, so the rule never fired for any input, and
//!   the paradigm it belongs to (`ase`, `ases`, `ásemos`, `asen`, all present)
//!   was missing exactly one member. `hablaseis` came back unstemmed.
//! * Italian [`crate::PorterStemmerIt`] listed **`"Yamo"`**, whose capital `Y`
//!   the Italian prelude cannot produce: it lowercases first and then writes
//!   only `I` and `U`.
//!
//! So this module does not sample. It enumerates **every entry of every table
//! in every language** and requires each one to be reachable.
//!
//! # What "reachable" means, precisely
//!
//! An entry `e` of language `L` is reachable when some word `w` exists that
//! (1) is a single token of `L`'s word walk and (2) satisfies
//! `prelude_L(w).ends_with(e)`. Both halves matter and each catches a
//! different defect: (1) is what `"  aseis"` fails, (2) is what `"Yamo"` fails.
//!
//! The search for such a `w` is not exhaustive — it cannot be — but it is
//! *constructive* and it is drawn from the language's own data. For each entry
//! the audit forms candidate tails (the entry as written, plus the entry with
//! each of the prelude's marker rewrites undone, so `aço~es` is also tried as
//! `ações` and `Ièrement` as `ièrement`) and prefixes each with every entry of
//! that language's own stop-word list, plus the empty prefix. One success
//! proves reachability; the audit only ever reports an entry when *every*
//! probe fails, which is a strong signal and, for the two entries above, a
//! proof — a space cannot appear in a token under any prefix, and a lowercasing
//! prelude cannot emit a capital under any prefix either.
//!
//! # Tables with no suffix semantics
//!
//! Two tables are not suffix alternations and get the check that fits them:
//!
//! * The **Lancaster** rule table is dispatched by the token's last character,
//!   so a rule whose pattern does not end in the character of the section it
//!   sits in can never be reached. That is checked directly, along with every
//!   rule's `size` against its own pattern.
//! * The **Carry** tables are exact-key maps read by binary search, so they
//!   must be sorted and free of duplicate keys or a lookup silently misses.
//!
//! [`crate::StemmerJa`] and [`crate::PorterStemmerFa`] have no rule table at
//! all — Japanese is one hard-coded rule and Persian stemming is the identity —
//! and [`crate::StemmerId`] is dictionary-driven rather than table-driven; its
//! roots are audited by `crate::id`'s own suite.

use crate::stopwords::Language;

use super::audit::is_one_whole_token;

/// One language's rule tables together with the prelude that runs before any
/// of them is searched.
struct LangTables {
    /// The language code, for failure messages.
    code: &'static str,
    /// Every rule table, named.
    tables: Vec<(&'static str, Vec<&'static str>)>,
    /// The prelude, in isolation — the transform `stem` applies to the whole
    /// token before the first table search.
    prelude: fn(&str) -> String,
    /// `(written, source)` for each unit the prelude writes: the audit undoes
    /// these to build a probe whose prelude output *reproduces* the entry.
    markers: &'static [(&'static str, &'static str)],
    /// Prefixes to try in front of each candidate tail.
    fillers: Vec<&'static str>,
}

/// Alphabetic entries of `lang`'s own stop-word list, used as probe prefixes.
///
/// Drawn from the language's data rather than invented so that every probe is
/// a plausible word of the right script, and so that both vowel-final and
/// consonant-final left contexts are covered without a hand-written list
/// claiming to know which letters those are.
fn fillers(entries: &'static [&'static str]) -> Vec<&'static str> {
    let mut out = vec![""];
    out.extend(
        entries
            .iter()
            .copied()
            .filter(|w| !w.is_empty() && w.chars().all(char::is_alphabetic)),
    );
    out
}

fn owned(
    tables: &'static [(&'static str, &'static [&'static str])],
) -> Vec<(&'static str, Vec<&'static str>)> {
    tables
        .iter()
        .map(|(name, entries)| (*name, entries.to_vec()))
        .collect()
}

fn all() -> Vec<LangTables> {
    vec![
        LangTables {
            code: "en",
            tables: crate::en::audit::tables(),
            prelude: crate::en::audit::prelude,
            markers: crate::en::audit::MARKERS,
            fillers: fillers(verbora_core::StopWordLanguage::En.stopwords()),
        },
        LangTables {
            code: "de",
            tables: owned(crate::de::audit::TABLES),
            prelude: crate::de::audit::prelude,
            markers: crate::de::audit::MARKERS,
            fillers: fillers(Language::De.defaults()),
        },
        LangTables {
            code: "es",
            tables: owned(crate::es::audit::TABLES),
            prelude: crate::es::audit::prelude,
            markers: crate::es::audit::MARKERS,
            fillers: fillers(Language::Es.defaults()),
        },
        LangTables {
            code: "fr",
            tables: owned(crate::fr::audit::TABLES),
            prelude: crate::fr::audit::prelude,
            markers: crate::fr::audit::MARKERS,
            fillers: fillers(Language::Fr.defaults()),
        },
        LangTables {
            code: "it",
            tables: owned(crate::it::audit::TABLES),
            prelude: crate::it::audit::prelude,
            markers: crate::it::audit::MARKERS,
            fillers: fillers(Language::It.defaults()),
        },
        LangTables {
            code: "nl",
            tables: owned(crate::nl::audit::TABLES),
            prelude: crate::nl::audit::prelude,
            markers: crate::nl::audit::MARKERS,
            fillers: fillers(Language::Nl.defaults()),
        },
        LangTables {
            code: "no",
            tables: owned(crate::no::audit::TABLES),
            prelude: crate::no::audit::prelude,
            markers: crate::no::audit::MARKERS,
            fillers: fillers(Language::No.defaults()),
        },
        LangTables {
            code: "pt",
            tables: owned(crate::pt::audit::TABLES),
            prelude: crate::pt::audit::prelude,
            markers: crate::pt::audit::MARKERS,
            fillers: fillers(Language::Pt.defaults()),
        },
        LangTables {
            code: "ru",
            tables: owned(crate::ru::audit::TABLES),
            prelude: crate::ru::audit::prelude,
            markers: crate::ru::audit::MARKERS,
            fillers: fillers(Language::Ru.defaults()),
        },
        LangTables {
            code: "sv",
            tables: owned(crate::sv::audit::TABLES),
            prelude: crate::sv::audit::prelude,
            markers: crate::sv::audit::MARKERS,
            fillers: fillers(Language::Sv.defaults()),
        },
        LangTables {
            code: "uk",
            tables: owned(crate::uk::audit::TABLES),
            prelude: crate::uk::audit::prelude,
            markers: crate::uk::audit::MARKERS,
            fillers: fillers(Language::Uk.defaults()),
        },
    ]
}

/// The candidate tails for one entry: the entry itself, and the entry with
/// each of the prelude's marker rewrites undone.
fn tails(entry: &str, markers: &[(&str, &str)]) -> Vec<String> {
    let mut out = vec![entry.to_owned()];
    for &(written, source) in markers {
        let undone = entry.replace(written, source);
        if undone != entry {
            out.push(undone);
        }
    }
    // Undoing every marker at once, for an entry that carries two kinds.
    let mut all = entry.to_owned();
    for &(written, source) in markers {
        all = all.replace(written, source);
    }
    if !out.contains(&all) {
        out.push(all);
    }
    out
}

/// Whether some probe word reaches `entry` at the point its table is searched.
fn reachable(entry: &str, lang: &LangTables) -> bool {
    for tail in tails(entry, lang.markers) {
        for filler in &lang.fillers {
            let probe = format!("{filler}{tail}");
            if !is_one_whole_token(&probe, false) {
                continue;
            }
            if (lang.prelude)(&probe).ends_with(entry) {
                return true;
            }
        }
    }
    false
}

/// Every entry of every suffix table, through the prelude that guards it.
///
/// The assertion is that **nothing** is unreachable. There is no exemption
/// list, deliberately: a stop-word list can legitimately carry a phrase or a
/// bare punctuation mark (they are data a caller may read), but a rule table
/// entry exists only to be matched, so an entry nothing can match is a defect
/// with no second reading available.
#[test]
fn every_rule_table_entry_is_reachable_through_its_own_prelude() {
    let mut audited = 0usize;
    let mut dead: Vec<String> = Vec::new();
    for lang in all() {
        for (name, entries) in &lang.tables {
            for entry in entries {
                audited += 1;
                if !reachable(entry, &lang) {
                    dead.push(format!("{}::{name}: {entry:?}", lang.code));
                }
            }
        }
    }
    assert!(
        dead.is_empty(),
        "{} of {audited} rule-table entries can never be matched by any word \
         of their own language: {dead:#?}",
        dead.len()
    );
    // Pinned by equality, not by a floor: a refactor that quietly stops
    // enumerating a table would otherwise report a clean sweep of nothing.
    assert_eq!(
        audited, 1210,
        "the number of rule-table entries this audit walks changed"
    );
}

/// No table lists the same entry twice.
///
/// A repeated entry is dead by construction: every search in this crate is
/// either longest-match or first-listed, and both settle on one of the two
/// copies for every possible input, so the other can never fire. It is also
/// how a table drifts — the four Ukrainian repeats below were all one letter
/// away from being the entry that was meant.
#[test]
fn no_rule_table_lists_the_same_entry_twice() {
    let mut repeats: Vec<String> = Vec::new();
    for lang in all() {
        for (name, entries) in &lang.tables {
            for (i, entry) in entries.iter().enumerate() {
                if entries[..i].contains(entry) {
                    repeats.push(format!("{}::{name}: {entry:?}", lang.code));
                }
            }
        }
    }
    assert!(
        repeats.is_empty(),
        "{} rule-table entries are listed twice, so the second copy can never \
         fire: {repeats:#?}",
        repeats.len()
    );
}

/// Every Lancaster rule sits in the section its own pattern selects.
///
/// [`crate::LancasterStemmer`] dispatches on the token's last character and
/// then walks that section in order, so a rule whose pattern ends in a
/// different character is unreachable however well-formed it looks. `size` is
/// checked against the pattern in the same pass: a rule that removes more
/// units than it matched would cut into the stem it was only supposed to
/// inspect.
///
/// **This test cannot see order, and its old name said otherwise.** It was
/// called `every_lancaster_rule_is_reachable_and_cuts_only_what_it_matched`,
/// and it passes every reorder of the table — five deliberate mutations,
/// including swaps that move real stems, left it green. Reachability in the
/// sense that matters here is a property of the *sequence*, because an earlier
/// rule shadows a later one; what this checks is that each rule is filed under
/// the section its pattern ends with. Both are worth checking. Only one of
/// them was being checked.
///
/// The sequence is pinned by
/// [`super::lancaster_rules::tests::every_rule_but_one_has_an_input_that_reaches_it`].
#[test]
fn every_lancaster_rule_is_filed_under_its_own_section_and_cuts_only_what_it_matched() {
    let mut audited = 0usize;
    let mut wrong_section: Vec<String> = Vec::new();
    let mut over_cut: Vec<String> = Vec::new();
    for last in 'a'..='z' {
        for rule in crate::data::lancaster_rules::section(last) {
            audited += 1;
            if !rule.pattern.ends_with(last) {
                wrong_section.push(format!("section {last:?}: {:?}", rule.pattern));
            }
            if rule.size > rule.pattern.len() {
                over_cut.push(format!(
                    "section {last:?}: {:?} removes {} of {} units",
                    rule.pattern,
                    rule.size,
                    rule.pattern.len()
                ));
            }
        }
    }
    assert!(
        wrong_section.is_empty(),
        "{} Lancaster rules sit in a section their pattern cannot reach: \
         {wrong_section:#?}",
        wrong_section.len()
    );
    assert!(
        over_cut.is_empty(),
        "{} Lancaster rules remove more than they matched: {over_cut:#?}",
        over_cut.len()
    );
    assert_eq!(
        audited, 115,
        "the number of Lancaster rules this audit walks changed"
    );
}

/// Every Carry table is sorted and free of duplicate keys.
///
/// [`crate::CarryStemmerFr`] looks its suffixes up with a binary search, which
/// silently misses on an unsorted table and silently prefers an arbitrary one
/// of two entries sharing a key.
#[test]
fn every_carry_table_is_a_well_formed_map() {
    let mut audited = 0usize;
    let mut unsorted: Vec<String> = Vec::new();
    for (s, step) in crate::data::carry_tables::STEPS.iter().enumerate() {
        for (t, table) in step.iter().enumerate() {
            audited += table.len();
            for pair in table.windows(2) {
                if pair[0].0 >= pair[1].0 {
                    unsorted.push(format!(
                        "step {s} table {t}: {:?} then {:?}",
                        pair[0].0, pair[1].0
                    ));
                }
            }
        }
    }
    assert!(
        unsorted.is_empty(),
        "{} Carry entries are out of order or duplicated, so a binary search \
         can miss them: {unsorted:#?}",
        unsorted.len()
    );
    assert_eq!(
        audited, 246,
        "the number of Carry entries this audit walks changed"
    );
}
