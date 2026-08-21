//! The stop-word contract, pinned.
//!
//! Two habits run through this file and are deliberate.
//!
//! **Enumerate, never sample.** A stop word that can never be matched fails
//! *silently*: it stays on the list, keeps answering `true` to
//! [`StopWordLanguage::is_stopword`], and simply never filters anything again.
//! The entries that die are exactly the ones a hand-written check does not
//! name. This workspace has already lost several that way — Dutch `"je "`
//! shipped with a trailing space, German `"ei,"` with a trailing comma, and a
//! Swedish diacritic fold killed 116 of 428 entries — so every list is walked
//! entry by entry.
//!
//! **One test owns the process-global list.** It is process-wide state and the
//! test harness runs tests on parallel threads, so every assertion about it
//! lives in a single function; everything else here uses the pure
//! [`StopWordLanguage`] API, which no mutation can reach.

use super::*;

/// The hand-written `ordinal` match indexes [`SORTED`], which is built from
/// [`StopWordLanguage::ALL`]; if the two ever disagreed, one language would
/// silently answer membership from another language's list.
#[test]
fn ordinal_agrees_with_the_language_table() {
    for (i, &lang) in StopWordLanguage::ALL.iter().enumerate() {
        assert_eq!(lang.ordinal(), i, "{}", lang.code());
    }
    assert_eq!(STOPWORD_LANGUAGES, &StopWordLanguage::ALL);
    // ...and the consequence that would follow if it did not: every list
    // answers for its own words and not for another language's.
    assert!(StopWordLanguage::Sv.is_stopword("och"));
    assert!(!StopWordLanguage::Id.is_stopword("och"));
    assert!(StopWordLanguage::Id.is_stopword("yang"));
    assert!(!StopWordLanguage::Sv.is_stopword("yang"));
}

#[test]
fn every_language_has_a_list_and_a_code() {
    assert_eq!(StopWordLanguage::ALL.len(), 16);
    for &lang in &StopWordLanguage::ALL {
        assert!(!lang.stopwords().is_empty(), "{} has no words", lang.code());
        assert_eq!(StopWordLanguage::from_code(lang.code()), Some(lang));
    }
    assert_eq!(StopWordLanguage::from_code("xx"), None);
    // Case-sensitive, as documented.
    assert_eq!(StopWordLanguage::from_code("EN"), None);
    assert_eq!(StopWordLanguage::from_code(""), None);
    // Codes are distinct.
    let mut codes: Vec<&str> = StopWordLanguage::ALL.iter().map(|l| l.code()).collect();
    codes.sort_unstable();
    codes.dedup();
    assert_eq!(codes.len(), 16);
}

/// The derived view is exactly `sort ∘ dedup` of the source list, checked for
/// every language rather than for a sample.
///
/// This is what replaced thirteen hand-written `SORTED_*` tables in
/// `verbora-stemmers`. Those tables were correct on the day they were typed;
/// nothing but a test stood between them and the source lists afterwards, and
/// the test they had asserted only that every source word was *findable* — not
/// that the sorted table held nothing else.
#[test]
fn the_sorted_view_is_derived_from_the_source_list() {
    for &lang in &StopWordLanguage::ALL {
        let mut expected = lang.stopwords().to_vec();
        expected.sort_unstable();
        expected.dedup();
        assert_eq!(lang.sorted(), &expected[..], "{}", lang.code());
        assert!(
            lang.sorted().windows(2).all(|w| w[0] < w[1]),
            "{} is not strictly sorted",
            lang.code()
        );
        // Every source word is findable, and the view holds nothing else.
        for word in lang.stopwords() {
            assert!(lang.is_stopword(word), "{} lost {word:?}", lang.code());
        }
        for word in lang.sorted() {
            assert!(
                lang.stopwords().contains(word),
                "{}: {word:?} is in the search view but not on the list",
                lang.code()
            );
        }
    }
}

/// The size and repetition of every list, pinned so that an edit to `data.rs`
/// has to be deliberate.
///
/// German's one duplicate is the repair of the dead entry `"ei,"`: the intended
/// word `ei` was already listed, so correcting the spelling left the word
/// listed twice rather than shrinking the list.
#[test]
fn recorded_lengths_and_duplicate_counts() {
    for (lang, len, dupes) in [
        (StopWordLanguage::En, 168, 0),
        (StopWordLanguage::De, 620, 1),
        (StopWordLanguage::Es, 70, 2),
        (StopWordLanguage::Fa, 26, 0),
        (StopWordLanguage::Fr, 168, 0),
        (StopWordLanguage::Id, 809, 0),
        (StopWordLanguage::It, 290, 0),
        (StopWordLanguage::Ja, 109, 0),
        (StopWordLanguage::Nl, 143, 1),
        (StopWordLanguage::No, 129, 2),
        (StopWordLanguage::Pl, 291, 1),
        (StopWordLanguage::Pt, 117, 0),
        (StopWordLanguage::Ru, 137, 8),
        (StopWordLanguage::Sv, 428, 0),
        (StopWordLanguage::Uk, 124, 6),
        (StopWordLanguage::Zh, 78, 0),
    ] {
        let words = lang.stopwords();
        assert_eq!(words.len(), len, "{} length", lang.code());
        assert_eq!(
            words.len() - lang.sorted().len(),
            dupes,
            "{} duplicate count",
            lang.code()
        );
    }
}

#[test]
fn membership_compares_scalar_sequences_exactly() {
    assert!(StopWordLanguage::En.is_stopword("the"));
    assert!(!StopWordLanguage::En.is_stopword("The"));
    assert!(!StopWordLanguage::En.is_stopword("THE"));
    assert!(!StopWordLanguage::En.is_stopword(" the"));
    assert!(!StopWordLanguage::En.is_stopword("the "));
    assert!(StopWordLanguage::Es.is_stopword("porque"));
    assert!(!StopWordLanguage::Es.is_stopword("PORQUE"));
    assert!(StopWordLanguage::Ru.is_stopword("и"));
    assert!(StopWordLanguage::Zh.is_stopword("的"));
    for &lang in &StopWordLanguage::ALL {
        assert!(
            !lang.is_stopword(""),
            "{} matched the empty string",
            lang.code()
        );
    }
}

#[test]
fn non_ascii_lists_survive_the_round_trip() {
    assert!(
        StopWordLanguage::Fa
            .stopwords()
            .iter()
            .any(|w| w.contains('ا'))
    );
    assert!(
        StopWordLanguage::Ja
            .stopwords()
            .iter()
            .any(|w| !w.is_ascii())
    );
    assert!(
        StopWordLanguage::Uk
            .stopwords()
            .iter()
            .any(|w| w.contains('і'))
    );
    // Astral scalars appear nowhere, and asking about one is not an error.
    for &lang in &StopWordLanguage::ALL {
        assert!(!lang.is_stopword("😀"));
    }
}

/// Normalisation is the caller's job, and the tables are all NFC so that doing
/// it in the obvious direction always agrees with them.
///
/// Enumerates every entry of every list. An entry in NFD would be a silent dead
/// entry for every caller who normalises to NFC — the same failure mode as a
/// stray trailing space, one layer down.
///
/// `verbora-core` deliberately has no Unicode dependency, so the property is
/// checked without a normalizer: NFC is idempotent and, for these tables,
/// every entry is already a sequence of NFC-stable scalars — no scalar in any
/// list has a canonical decomposition. That is checked here by rejecting the
/// combining marks (`U+0300`–`U+036F`, and the Cyrillic pair `U+0483`–`U+0489`)
/// that a decomposed spelling of any word on these lists would have to use.
/// `verbora-util`'s own suite runs the same lists through
/// `unicode_normalization` and reaches the same verdict from the other side.
#[test]
fn every_entry_is_already_nfc() {
    let mut checked = 0usize;
    for &lang in &StopWordLanguage::ALL {
        for &entry in lang.stopwords() {
            for c in entry.chars() {
                assert!(
                    !matches!(c, '\u{0300}'..='\u{036F}' | '\u{0483}'..='\u{0489}'),
                    "{}: {entry:?} carries the combining mark {c:?}, so it is not NFC",
                    lang.code()
                );
            }
            checked += 1;
        }
    }
    let total: usize = StopWordLanguage::ALL
        .iter()
        .map(|l| l.stopwords().len())
        .sum();
    assert_eq!(checked, total);
    assert_eq!(total, 3_707);

    // The consequence a caller can observe: the decomposed spelling of a listed
    // word is a different string and is not on the list.
    assert!(StopWordLanguage::Fr.is_stopword("été"));
    assert!(!StopWordLanguage::Fr.is_stopword("e\u{0301}te\u{0301}"));
}

/// The entries of one list that cannot be a token however the caller tokenizes,
/// split by the reason.
struct Unmatchable {
    /// Entries with no alphanumeric scalar anywhere — lone punctuation and
    /// symbols. A word token holds a letter or a digit by definition, so
    /// nothing a word tokenizer emits can equal these.
    symbols: &'static [&'static str],
    /// Entries made of two or more space-separated words. A word tokenizer
    /// breaks at whitespace unconditionally, so the phrase as spelled is never
    /// the string membership is asked about.
    phrases: &'static [&'static str],
}

const NOTHING: Unmatchable = Unmatchable {
    symbols: &[],
    phrases: &[],
};

/// Every entry of every list, checked against the shape a token has.
///
/// Deliberately not a sample: the entries that go dead are exactly the ones a
/// spot check does not name. `verbora-core` sits below `verbora-tokenizers` and
/// cannot call one, so the rule applied is the *necessary* half of the token
/// definition, which needs no tokenizer to state — a UAX #29 word segment
/// containing a letter or a digit holds no whitespace and neither begins nor
/// ends with punctuation. `verbora-stemmers`' `data::audit` walks the same
/// lists through the real tokenizer and the real stemming pipelines; the two
/// agree because they check the same property from opposite sides.
///
/// Three failures are possible and only the first two can be excused:
///
/// * an entry with no word in it at all (`"_"`) — excusable, listed;
/// * a genuine multi-word phrase (`"с кем"`) — excusable, listed;
/// * **a word carrying a stray character** (`"je "`, `"ei,"`) — never
///   excusable, because it is always a misspelling of a word that was meant to
///   be filtered and never was.
///
/// English's `symbols` set is empty, and that is the point of pinning it:
/// `"$"` and `"_"` were on that list until 2026-08 and neither could ever be
/// tested against it, in any pipeline. The other fourteen non-empty sets are
/// **recorded, not endorsed** — the same argument retires them, and doing so is
/// a change to lists two other crates pin the contents of.
#[test]
fn entries_are_shaped_like_tokens() {
    for (lang, expected) in [
        (StopWordLanguage::En, NOTHING),
        (StopWordLanguage::De, NOTHING),
        (
            StopWordLanguage::Es,
            Unmatchable {
                symbols: &["_"],
                phrases: &[],
            },
        ),
        (
            StopWordLanguage::Fa,
            Unmatchable {
                symbols: &["؟", "!", "٪", ".", "،", "؛", ":", ";", ","],
                phrases: &[],
            },
        ),
        (StopWordLanguage::Fr, NOTHING),
        (StopWordLanguage::Id, NOTHING),
        (
            StopWordLanguage::It,
            Unmatchable {
                symbols: &["_"],
                phrases: &[],
            },
        ),
        (StopWordLanguage::Ja, NOTHING),
        (
            StopWordLanguage::Nl,
            Unmatchable {
                symbols: &["$", "_", "-"],
                phrases: &[],
            },
        ),
        (
            StopWordLanguage::No,
            Unmatchable {
                symbols: &["_"],
                phrases: &[],
            },
        ),
        (
            StopWordLanguage::Pl,
            Unmatchable {
                symbols: &["$", "_"],
                phrases: &[],
            },
        ),
        (
            StopWordLanguage::Pt,
            Unmatchable {
                symbols: &["_"],
                phrases: &[],
            },
        ),
        (
            StopWordLanguage::Ru,
            Unmatchable {
                symbols: &["$", "_"],
                phrases: &["может быть", "все еще", "с кем", "хотел бы"],
            },
        ),
        (StopWordLanguage::Sv, NOTHING),
        (
            StopWordLanguage::Uk,
            Unmatchable {
                symbols: &["$", "_"],
                phrases: &["може бути", "все ще", "хотів би"],
            },
        ),
        (StopWordLanguage::Zh, NOTHING),
    ] {
        let mut symbols = Vec::new();
        let mut phrases = Vec::new();
        let mut stray = Vec::new();

        for &entry in lang.stopwords() {
            assert!(!entry.is_empty(), "{} holds an empty entry", lang.code());
            if !entry.chars().any(char::is_alphanumeric) {
                symbols.push(entry);
                continue;
            }
            let words: Vec<&str> = entry.split_whitespace().collect();
            // `join` catches leading, trailing and repeated whitespace, which
            // is how `"je "` died.
            let well_formed = words.join(" ") == entry
                && words.iter().all(|word| {
                    word.chars().next().is_some_and(char::is_alphanumeric)
                        && word.chars().next_back().is_some_and(char::is_alphanumeric)
                });
            if !well_formed {
                stray.push(entry);
            } else if words.len() > 1 {
                phrases.push(entry);
            }
        }

        assert!(
            stray.is_empty(),
            "{}: {} entries hold a word plus characters no token can carry, so \
             nothing will ever match them: {stray:?}. Correct the spelling — \
             this class has no exemption.",
            lang.code(),
            stray.len()
        );
        assert_eq!(
            symbols,
            expected.symbols,
            "{}: the set of entries holding no word at all changed",
            lang.code()
        );
        assert_eq!(
            phrases,
            expected.phrases,
            "{}: the set of multi-word entries changed",
            lang.code()
        );
        for entry in symbols.iter().chain(phrases.iter()) {
            assert!(
                lang.is_stopword(entry),
                "{}: {entry:?} is not found by its own membership test",
                lang.code()
            );
        }
    }
}

/// The entries earlier audits found dead, stated as the behaviour a caller
/// sees.
///
/// `"ei,"` is the reason the audit above asks about *shape* rather than only
/// about membership: German also lists `"ei"`, so any test that tokenized
/// German text and checked what survived would have seen `ei` filtered and
/// concluded the list was fine.
///
/// `"$"` and `"_"` are the English pair this crate retired, and they show the
/// third shape the same defect takes: an entry that is not a misspelling of
/// anything, just a string no tokenizer can produce. Single letters and digits
/// stay, because those *are* word tokens.
#[test]
fn the_entries_that_were_dead_are_gone() {
    assert!(StopWordLanguage::Nl.is_stopword("je"));
    assert!(!StopWordLanguage::Nl.is_stopword("je "));
    assert!(StopWordLanguage::De.is_stopword("ei"));
    assert!(!StopWordLanguage::De.is_stopword("ei,"));

    assert!(!StopWordLanguage::En.is_stopword("$"));
    assert!(!StopWordLanguage::En.is_stopword("_"));
    assert!(StopWordLanguage::En.is_stopword("a"));
    assert!(StopWordLanguage::En.is_stopword("z"));
    assert!(StopWordLanguage::En.is_stopword("0"));
    assert!(StopWordLanguage::En.is_stopword("9"));
}

// ---------------------------------------------------------------------------
// StopWords, the owned list
// ---------------------------------------------------------------------------

#[test]
fn an_empty_list_contains_nothing() {
    let s = StopWords::new();
    assert!(s.is_empty());
    assert_eq!(s.len(), 0);
    assert!(!s.contains(""));
    assert!(!s.contains("the"));
    assert_eq!(s.words(), &[] as &[String]);
}

#[test]
fn add_appends_unconditionally_and_remove_takes_one_occurrence() {
    let mut s = StopWords::new();
    s.add("dup");
    s.add("dup");
    assert_eq!(s.words(), &["dup", "dup"]);
    assert!(s.contains("dup"));

    assert!(s.remove("dup"));
    assert_eq!(s.words(), &["dup"]);
    assert!(s.contains("dup"), "one occurrence is still on the list");

    assert!(s.remove("dup"));
    assert!(!s.contains("dup"));
    assert!(s.is_empty());

    // A remove that found nothing says so rather than looking like a success.
    assert!(!s.remove("dup"));
    assert!(!s.remove("never-present"));
}

#[test]
fn remove_all_reports_how_many_it_found() {
    let mut s = StopWords::from_iter_of(["a", "b", "c"]);
    assert_eq!(s.remove_all(["a", "zzz", "c"]), 2);
    assert_eq!(s.words(), &["b"]);
    assert_eq!(s.remove_all(std::iter::empty()), 0);
}

#[test]
fn for_language_is_an_independent_copy() {
    let mut s = StopWords::for_language(StopWordLanguage::Fr);
    assert_eq!(s.len(), StopWordLanguage::Fr.stopwords().len());
    assert!(s.contains("être"));
    s.add("verbora");
    assert!(s.contains("verbora"));
    // The shipped list is untouched, and so is another copy.
    assert!(!StopWordLanguage::Fr.is_stopword("verbora"));
    assert!(!StopWords::for_language(StopWordLanguage::Fr).contains("verbora"));
    // ...and every shipped entry survived the copy.
    for entry in StopWordLanguage::Fr.stopwords() {
        assert!(s.contains(entry), "{entry:?} was lost in the copy");
    }
}

#[test]
fn collect_and_from_iter_of_build_the_same_list() {
    let a = StopWords::from_iter_of(["the", "a"]);
    let b: StopWords = ["the", "a"].into_iter().collect();
    let c: StopWords = vec![String::from("the"), String::from("a")]
        .into_iter()
        .collect();
    assert_eq!(a, b);
    assert_eq!(a, c);
}

#[test]
fn unicode_words_round_trip_through_an_owned_list() {
    let mut s = StopWords::from_iter_of(["café", "日本語", "😀"]);
    assert!(s.contains("café"));
    assert!(s.contains("日本語"));
    assert!(s.contains("😀"));
    // NFD is a different string, as documented.
    assert!(!s.contains("cafe\u{0301}"));
    assert!(s.remove("😀"));
    assert!(!s.contains("😀"));
}

// ---------------------------------------------------------------------------
// The process-global list — one test, because the state is process-wide
// ---------------------------------------------------------------------------

/// Everything the process-global English list promises, in one function.
///
/// # Why one function
///
/// The list is a `static`, and the harness runs `#[test]`s on parallel threads.
/// Two tests that each mutate it would interleave and fail each other at
/// random. Serialising them into one call sequence is the only way to assert
/// anything about a global without a lock the API does not have.
///
/// # The poisoning half, and what it fails against
///
/// `RwLock` poisons when a panic unwinds through a held guard, and every
/// mutator here takes the same lock [`is_global_stopword`] reads through. This
/// crate used to propagate that poison with `.expect("stop-word lock
/// poisoned")` on **every** entry point — so one panic anywhere near a
/// mutation turned every subsequent membership test in the process into a
/// panic, and `is_global_stopword` is on the hot path of every token of every
/// document an English stemmer filters. A caller's own `Into<String>` is enough
/// to reach it.
///
/// The probe below installs a poisoned lock directly (a panicking closure
/// inside the write guard, caught) and then asks the same questions a document
/// would. Against the unfixed code every assertion after the probe panics
/// instead of failing; against this one they pass. `verbora-stemmers` made the
/// same repair to its own per-language lists; this crate held the last copy of
/// the defect.
#[test]
fn the_process_global_list_behaves_as_documented() {
    reset_global_stopwords();

    // The lock-free path: no mutation has happened, so this is the shipped
    // list and nothing else.
    assert!(is_global_stopword("the"));
    assert!(!is_global_stopword("The"));
    assert!(!is_global_stopword(""));
    assert_eq!(global_stopwords().len(), 168);
    assert_eq!(
        global_stopwords(),
        StopWordLanguage::En.stopwords().to_vec(),
        "the untouched snapshot is the shipped list"
    );

    // A mutation is visible to the global reader and invisible to the pure one.
    add_global_stopword("verbora");
    assert!(is_global_stopword("verbora"));
    assert!(!StopWordLanguage::En.is_stopword("verbora"));
    assert_eq!(global_stopwords().len(), 169);
    assert_eq!(
        global_stopwords().last().map(String::as_str),
        Some("verbora")
    );

    assert!(remove_global_stopword("the"));
    assert!(!is_global_stopword("the"));
    assert!(
        StopWordLanguage::En.is_stopword("the"),
        "the shipped list is a pure function of the data"
    );
    assert!(!remove_global_stopword("the"), "it is already gone");

    add_global_stopwords(["alpha", "beta"]);
    assert!(is_global_stopword("alpha") && is_global_stopword("beta"));
    assert_eq!(remove_global_stopwords(["alpha", "beta", "gamma"]), 2);
    assert!(!is_global_stopword("alpha"));

    // Reset puts every shipped entry back and drops every addition.
    reset_global_stopwords();
    assert!(!is_global_stopword("verbora"));
    for entry in StopWordLanguage::En.stopwords() {
        assert!(is_global_stopword(entry), "{entry:?} did not come back");
    }

    // --- every state of the flag/list pair answers with a whole list ------
    //
    // A reader samples `GLOBAL_MUTATED` and `GLOBAL` at different instants, so
    // it can catch either mixed state while a writer is mid-flight. The window
    // is not the thing to reproduce — spawning threads and hoping one lands in
    // a few instructions is a lottery that reports "pass" on every ticket it
    // loses. The *states* are. All four are installed here directly, from one
    // thread, and each is checked against **every** entry of the list, because
    // the failure this guards against was uniform: `verbora-stemmers` shipped a
    // per-language list where one of these states reported the whole language
    // as containing no stop words at all.
    let install = |mutated: bool, list: StopWords| {
        let mut guard = GLOBAL.write().unwrap_or_else(PoisonError::into_inner);
        GLOBAL_MUTATED.store(mutated, Ordering::Relaxed);
        *guard = list;
        drop(guard);
    };
    let shipped = || StopWords::for_language(StopWordLanguage::En);
    let mut extended = shipped();
    extended.add("zzz-state-probe");

    for (state, mutated, list, probe_visible) in [
        ("flag clear, shipped list", false, shipped(), false),
        // A `reset` that has cleared the flag but not yet replaced the list:
        // the flag is the authority, so the extra word must not be visible.
        ("flag clear, mutated list", false, extended.clone(), false),
        // An `add` that has taken the lock but not yet stored the flag.
        ("flag set, shipped list", true, shipped(), false),
        ("flag set, mutated list", true, extended.clone(), true),
    ] {
        install(mutated, list);
        for entry in StopWordLanguage::En.stopwords() {
            assert!(
                is_global_stopword(entry),
                "{state}: {entry:?} vanished from the English list"
            );
        }
        assert_eq!(
            is_global_stopword("zzz-state-probe"),
            probe_visible,
            "{state}: the addition's visibility is wrong"
        );
        assert!(global_stopwords().len() >= 168, "{state}: short snapshot");
    }
    reset_global_stopwords();

    // --- poisoning -------------------------------------------------------
    let poisoned = std::panic::catch_unwind(|| {
        let _guard = GLOBAL.write().expect("first acquisition succeeds");
        panic!("a caller's own code panicked under the lock");
    });
    assert!(poisoned.is_err(), "the probe did not panic");
    assert!(
        GLOBAL.is_poisoned(),
        "the probe did not poison the lock, so this test proves nothing"
    );

    // Force the locked path — the fast path would skip the lock entirely and
    // prove nothing about poisoning.
    add_global_stopword("zzz-poison-probe");
    assert!(is_global_stopword("zzz-poison-probe"));
    for entry in StopWordLanguage::En.stopwords() {
        assert!(
            is_global_stopword(entry),
            "{entry:?} vanished with the poison"
        );
    }
    assert_eq!(global_stopwords().len(), 169);
    assert!(remove_global_stopword("zzz-poison-probe"));
    reset_global_stopwords();
    assert!(is_global_stopword("the"), "the list survived the reset");

    GLOBAL.clear_poison();
    assert!(!GLOBAL.is_poisoned());
}
