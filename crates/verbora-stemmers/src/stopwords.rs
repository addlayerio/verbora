//! Per-language stop-word lists.
//!
//! The module is private; the whole contract lives on [`Language`], which the
//! crate root re-exports.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{LazyLock, PoisonError, RwLock};

use verbora_core::{StopWordLanguage, StopWords};

/// A language this crate carries a stop-word list for.
///
/// The variant is the handle to the list: [`Self::defaults`] reads the shipped
/// one, [`Self::contains`] and [`Self::words`] read the current one, and
/// [`Self::add`], [`Self::add_all`], [`Self::remove`], [`Self::remove_all`]
/// and [`Self::reset`] change it.
///
/// English is not a variant here. It lives in [`verbora_core`] behind the
/// process-global list — [`crate::PorterStemmer`] and
/// [`crate::LancasterStemmer`] here, and the phonetics helpers there, all read
/// the same mutable list, so it needs one home and a lock rather than a copy
/// per crate.
///
/// The **data** for the thirteen languages below lives in `verbora-core` too,
/// and only there. This crate used to ship a byte-identical second copy of
/// every list plus a hand-written sorted table beside each one; nothing in the
/// build made the copies agree, which is exactly how the Dutch `"je "` typo
/// came to live in two places at once. What remains here is the *mutability* —
/// the per-language global list and the flag that gates it — which is this
/// crate's own behaviour rather than shared data.
///
/// # The lists are process-global
///
/// Mutation is **not** scoped to a stemmer value, a thread or a call: the
/// lists live in statics, so `Language::De.add("foo")` changes the answer
/// every [`crate::PorterStemmerDe`] in the process gets, for the rest of the
/// process. That is a deliberate shape rather than an accident — the lists are
/// large, shared, and read on every token of every document — but it makes
/// [`Self::reset`] the only way for a test to isolate itself, and it means a
/// library that mutates a list is changing its caller's behaviour.
///
/// Until something mutates a list, membership is a binary search over static
/// data with no lock, no allocation and no lazy initialisation, so the cost of
/// this flexibility is paid only by programs that use it.
///
/// # What a concurrent reader can observe
///
/// Each language's state is two words: a has-ever-been-mutated flag, and the
/// list itself behind a lock. [`Self::contains`] and [`Self::words`] sample
/// those two at different instants, so a reader running alongside a mutator
/// can catch any of the four combinations — and **each one answers with a
/// whole list**:
///
/// | flag | list | what a reader sees |
/// |---|---|---|
/// | clear | absent | the defaults — nothing has happened |
/// | clear | present | the defaults; the flag is the authority |
/// | set | absent | the defaults — a [`Self::reset`] is in flight |
/// | set | present | the mutated list |
///
/// Row three is the one that was wrong. [`Self::contains`] answered it with
/// `is_some_and`, which is `false` for **every** word, so a `contains` racing
/// a `reset` reported the whole language as containing no stop words at all —
/// not a stale answer, but an answer no state of the list ever had. It now
/// falls back to the defaults, which is what the reset is on its way to
/// installing, so the intermediate state is observationally identical to the
/// finished one.
///
/// Rows two and three are transient rather than permanent only because every
/// flag transition happens while the write lock is held. Without that, a
/// [`Self::reset`] releasing the lock before clearing the flag could have its
/// clear land *after* a concurrent [`Self::add`] set it, leaving a cleared flag
/// beside a list that does contain the added word — the lock-free path reading
/// defaults forever and the addition lost with no way to observe it.
///
/// # Not every language exposes every mutator through its stemmer
///
/// The methods here are uniform, but the per-stemmer conveniences are not:
/// [`crate::PorterStemmer`], [`crate::LancasterStemmer`] and
/// [`crate::StemmerId`] carry all four, [`crate::PorterStemmerNo`] and
/// [`crate::PorterStemmerSv`] carry only the two adders,
/// [`crate::PorterStemmerPt`] only `add_all`, and the rest carry none. Any
/// list can still be changed through this type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Language {
    /// German (620 words).
    De,
    /// Spanish (70 words).
    Es,
    /// Persian (26 words).
    Fa,
    /// French (168 words).
    Fr,
    /// Indonesian (809 words).
    Id,
    /// Italian (290 words).
    It,
    /// Japanese (109 words).
    ///
    /// **No stemmer consults this list.** [`crate::StemmerJa`] stems one token
    /// and does not tokenize — UAX #29's default rules do not segment
    /// Japanese, so the caller supplies the segmentation — which means it
    /// implements no [`crate::TokenizeAndStem`] and applies no stop-word
    /// filter. The list is kept and stays reachable through [`Self::contains`]
    /// and [`Self::words`], for a caller who has segmented the text
    /// themselves.
    Ja,
    /// Dutch (143 words).
    Nl,
    /// Norwegian (129 words).
    No,
    /// Portuguese (117 words).
    Pt,
    /// Russian (137 words).
    Ru,
    /// Swedish (428 words).
    Sv,
    /// Ukrainian (124 words).
    Uk,
}

impl Language {
    /// Every language this module carries a list for.
    pub const ALL: [Self; 13] = [
        Self::De,
        Self::Es,
        Self::Fa,
        Self::Fr,
        Self::Id,
        Self::It,
        Self::Ja,
        Self::Nl,
        Self::No,
        Self::Pt,
        Self::Ru,
        Self::Sv,
        Self::Uk,
    ];

    /// The shipped list, in source order, ignoring any mutation.
    ///
    /// Borrowed straight from `verbora-core`, which is the single home of this
    /// data.
    pub const fn defaults(self) -> &'static [&'static str] {
        self.shared().stopwords()
    }

    /// This language as `verbora-core` names it.
    ///
    /// The thirteen variants here are a subset of that crate's sixteen: it also
    /// carries English, Polish and Chinese, none of which this crate stems.
    const fn shared(self) -> StopWordLanguage {
        match self {
            Self::De => StopWordLanguage::De,
            Self::Es => StopWordLanguage::Es,
            Self::Fa => StopWordLanguage::Fa,
            Self::Fr => StopWordLanguage::Fr,
            Self::Id => StopWordLanguage::Id,
            Self::It => StopWordLanguage::It,
            Self::Ja => StopWordLanguage::Ja,
            Self::Nl => StopWordLanguage::Nl,
            Self::No => StopWordLanguage::No,
            Self::Pt => StopWordLanguage::Pt,
            Self::Ru => StopWordLanguage::Ru,
            Self::Sv => StopWordLanguage::Sv,
            Self::Uk => StopWordLanguage::Uk,
        }
    }

    /// Whether `word` is currently a stop word in this language.
    ///
    /// Until something mutates the list this is a binary search over static
    /// data: no lock, no allocation, no lazy initialisation.
    #[must_use]
    pub fn contains(self, word: &str) -> bool {
        contains(self, word)
    }

    /// A snapshot of the current list, in source order.
    #[must_use]
    pub fn words(self) -> Vec<String> {
        words(self)
    }

    /// Appends one stop word to this language's process-global list.
    pub fn add(self, word: impl Into<String>) {
        add(self, word);
    }

    /// Appends several stop words to this language's process-global list.
    pub fn add_all<I, S>(self, more: I)
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        add_all(self, more);
    }

    /// Removes the **first** occurrence of `word`.
    pub fn remove(self, word: &str) {
        remove(self, word);
    }

    /// Removes the first occurrence of each of `more`.
    pub fn remove_all<'a, I>(self, more: I)
    where
        I: IntoIterator<Item = &'a str>,
    {
        remove_all(self, more);
    }

    /// Restores this language's list to [`Self::defaults`].
    ///
    /// Every mutator above is process-global, so a test that adds a stop word
    /// changes the answer every other caller in the process gets. This is the
    /// way back, and it has no equivalent among the mutators: it is Verbora's
    /// own, added so that a test can isolate itself.
    pub fn reset(self) {
        reset(self);
    }

    const fn index(self) -> usize {
        match self {
            Self::De => 0,
            Self::Es => 1,
            Self::Fa => 2,
            Self::Fr => 3,
            Self::Id => 4,
            Self::It => 5,
            Self::Ja => 6,
            Self::Nl => 7,
            Self::No => 8,
            Self::Pt => 9,
            Self::Ru => 10,
            Self::Sv => 11,
            Self::Uk => 12,
        }
    }
}

/// One entry per language. `MUTATED` gates the lock entirely: until a program
/// mutates a list, membership is a binary search over a static slice with no
/// locking, no allocation, and no lazy initialisation of the `RwLock` payload.
///
/// This is the flag half of the pair whose four observable states [`Language`]
/// tabulates; read that table before changing any store here.
/// Flag transitions happen while the write lock is held (see [`with_list`] and
/// [`reset`]), which is what keeps the two mixed states transient rather than
/// permanent.
///
/// `Relaxed` remains sufficient. The flag only ever selects between two
/// answers that are each correct for some instant of the timeline, and the
/// `RwLock` supplies the happens-before for the payload itself.
static MUTATED: [AtomicBool; 13] = [const { AtomicBool::new(false) }; 13];

#[allow(
    clippy::type_complexity,
    reason = "one lazily-built list per language; naming the array adds no clarity"
)]
static GLOBAL: LazyLock<[RwLock<Option<StopWords>>; 13]> =
    LazyLock::new(|| std::array::from_fn(|_| RwLock::new(None)));

/// Runs `f` against the language's live list, under the write lock.
///
/// # Poisoning is recovered from, not propagated
///
/// A poisoned lock would make **every** later stop-word operation in the
/// process panic — including [`contains`], which runs on every token of every
/// document — so a single panic anywhere near a mutation would turn every
/// subsequent `tokenize_and_stem` call into a panic. That is a far worse
/// failure than the one it reports, and the state it guards does not need it:
/// the payload is a list of strings whose every intermediate value is a valid
/// list, so there is no invariant a partially-applied mutation could have
/// broken. Callers therefore see the list as it stands.
///
/// The mutators below also do all of their *caller-supplied* work before
/// taking the lock — a user `Into<String>` or `IntoIterator` implementation
/// that panics is the only realistic way to poison this lock — so in practice
/// the recovery is a second line of defence rather than the first.
fn with_list<R>(lang: Language, f: impl FnOnce(&mut StopWords) -> R) -> R {
    let mut guard = GLOBAL[lang.index()]
        .write()
        .unwrap_or_else(PoisonError::into_inner);
    let list =
        guard.get_or_insert_with(|| StopWords::from_iter_of(lang.defaults().iter().copied()));
    let out = f(list);
    // Published while `guard` is still alive, so this cannot interleave with a
    // `reset` clearing the same flag; see [`MUTATED`]'s note.
    MUTATED[lang.index()].store(true, Ordering::Relaxed);
    drop(guard);
    out
}

/// Whether `word` is currently a stop word in `lang`.
fn contains(lang: Language, word: &str) -> bool {
    if !MUTATED[lang.index()].load(Ordering::Relaxed) {
        return lang.shared().is_stopword(word);
    }
    GLOBAL[lang.index()]
        .read()
        .unwrap_or_else(PoisonError::into_inner)
        .as_ref()
        // An empty payload means the list is the default one — either nothing
        // has built it yet, or a `reset` has cleared it and this read landed
        // before the flag caught up. Answering `false` here instead would
        // report every word of the language as not-a-stop-word; see
        // [`MUTATED`].
        .map_or_else(|| lang.shared().is_stopword(word), |l| l.contains(word))
}

/// Appends one stop word.
///
/// The conversion runs *before* the lock is taken: `Into<String>` is the
/// caller's code, and running it under a write lock would let a panicking
/// implementation poison a lock every stop-word lookup in the process goes
/// through.
fn add(lang: Language, word: impl Into<String>) {
    let word = word.into();
    with_list(lang, |l| l.add(word));
}

/// Appends several stop words.
///
/// The iterator is drained before the lock is taken, for the reason [`add`]
/// gives.
fn add_all<I, S>(lang: Language, words: I)
where
    I: IntoIterator<Item = S>,
    S: Into<String>,
{
    let words: Vec<String> = words.into_iter().map(Into::into).collect();
    with_list(lang, |l| l.add_all(words));
}

/// Removes the **first** occurrence of `word`.
fn remove(lang: Language, word: &str) {
    with_list(lang, |l| l.remove(word));
}

/// Removes the first occurrence of each of `words`.
///
/// The iterator is drained before the lock is taken, for the reason [`add`]
/// gives.
fn remove_all<'a, I>(lang: Language, words: I)
where
    I: IntoIterator<Item = &'a str>,
{
    let words: Vec<&'a str> = words.into_iter().collect();
    with_list(lang, |l| l.remove_all(words));
}

/// A snapshot of the current list, in order.
fn words(lang: Language) -> Vec<String> {
    if !MUTATED[lang.index()].load(Ordering::Relaxed) {
        return lang.defaults().iter().map(|s| (*s).to_owned()).collect();
    }
    GLOBAL[lang.index()]
        .read()
        .unwrap_or_else(PoisonError::into_inner)
        .as_ref()
        .map_or_else(
            || lang.defaults().iter().map(|s| (*s).to_owned()).collect(),
            |l| l.words().to_vec(),
        )
}

/// Restores a language's list to its default.
///
/// Both halves of the reset happen under the write lock, and the
/// has-ever-been-mutated flag is cleared before the list is. Neither is
/// cosmetic: releasing the lock between them let a concurrent [`add`] set the
/// flag in the gap and then lose its word to this function's clear, and
/// clearing the list first left a window in which [`contains`] saw a set flag
/// beside an absent list. [`Language`]'s own documentation tabulates the four
/// states a concurrent reader can catch and what each must answer with; the
/// test module walks all four.
fn reset(lang: Language) {
    let mut guard = GLOBAL[lang.index()]
        .write()
        .unwrap_or_else(PoisonError::into_inner);
    MUTATED[lang.index()].store(false, Ordering::Relaxed);
    *guard = None;
    drop(guard);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_have_the_documented_sizes() {
        assert_eq!(Language::De.defaults().len(), 620);
        assert_eq!(Language::Es.defaults().len(), 70);
        assert_eq!(Language::Fa.defaults().len(), 26);
        assert_eq!(Language::Fr.defaults().len(), 168);
        assert_eq!(Language::Id.defaults().len(), 809);
        assert_eq!(Language::It.defaults().len(), 290);
        assert_eq!(Language::Ja.defaults().len(), 109);
        assert_eq!(Language::Nl.defaults().len(), 143);
        assert_eq!(Language::No.defaults().len(), 129);
        assert_eq!(Language::Pt.defaults().len(), 117);
        assert_eq!(Language::Ru.defaults().len(), 137);
        assert_eq!(Language::Sv.defaults().len(), 428);
        assert_eq!(Language::Uk.defaults().len(), 124);
    }

    /// Every entry of every list is found by the lock-free path, and the
    /// thirteen variants here map onto the right `verbora-core` language.
    ///
    /// The mapping is the part that can silently go wrong: [`Language::shared`]
    /// is hand-written, and swapping two arms would make one language answer
    /// membership from another's list without any count changing. So this
    /// enumerates every entry of every list, and then checks that no language
    /// claims a word only its neighbours have.
    #[test]
    fn every_entry_is_found_through_the_shared_data() {
        for lang in Language::ALL {
            assert_eq!(
                lang.defaults(),
                lang.shared().stopwords(),
                "{lang:?} is wired to the wrong shared list"
            );
            assert_eq!(lang.shared().code(), lang_code(lang));
            for w in lang.defaults() {
                assert!(contains(lang, w), "{w:?} is missing from {lang:?}");
            }
        }
        // A word from one list is not on the others' — the consequence a
        // swapped arm would have.
        assert!(contains(Language::Sv, "och"));
        assert!(!contains(Language::Id, "och"));
        assert!(contains(Language::Id, "yang"));
        assert!(!contains(Language::Sv, "yang"));
    }

    /// The ISO 639-1 code each variant stands for, written out here so that
    /// [`Language::shared`] is checked against something independent of it.
    const fn lang_code(lang: Language) -> &'static str {
        match lang {
            Language::De => "de",
            Language::Es => "es",
            Language::Fa => "fa",
            Language::Fr => "fr",
            Language::Id => "id",
            Language::It => "it",
            Language::Ja => "ja",
            Language::Nl => "nl",
            Language::No => "no",
            Language::Pt => "pt",
            Language::Ru => "ru",
            Language::Sv => "sv",
            Language::Uk => "uk",
        }
    }

    /// Every combination of the two words a reader samples independently
    /// answers with a whole list.
    ///
    /// # Why this is not a thread test
    ///
    /// The defect was a window inside [`reset`]: between clearing the payload
    /// and clearing the flag, [`contains`] took the locked path, found `None`
    /// and reported **every** word of the language as not-a-stop-word. Spawning
    /// threads and hoping one lands in a window a few instructions wide is not
    /// a test — it is a lottery that reports "pass" on every ticket it loses,
    /// and it would keep reporting "pass" after the bug came back.
    ///
    /// The window is not the thing to reproduce; the *state* is. `MUTATED` and
    /// `GLOBAL` have four combinations between them, a concurrent observer can
    /// catch any of them, and each has exactly one right answer — so this
    /// installs all four directly, from one thread, and asks. No timing, no
    /// scheduler, no flake, and the assertion fails on the unfixed code every
    /// single run.
    ///
    /// Enumerating rather than sampling applies here too: each state is checked
    /// against **every** entry of the list, not a probe word, because the
    /// failure was uniform — it was the whole language that went missing.
    #[test]
    fn no_state_of_the_pair_reports_an_empty_list() {
        // Japanese consults no stemmer pipeline, so nothing else in the suite
        // can be mid-lookup in this list while the states below are installed.
        let lang = Language::Ja;
        let i = lang.index();
        let entries = lang.defaults();
        let probe = "zzz-probe-not-a-japanese-word";

        let assert_defaults = |state: &str| {
            for w in entries {
                assert!(
                    contains(lang, w),
                    "{state}: {w:?} vanished from the Japanese list"
                );
            }
            assert!(!contains(lang, probe), "{state}: {probe:?} appeared");
        };

        // (false, None) — untouched.
        reset(lang);
        assert_defaults("flag clear, payload empty");

        // (true, None) — the state `reset` passes through, and the one that
        // used to answer `false` for every word in the language.
        *GLOBAL[i].write().expect("lock poisoned") = None;
        MUTATED[i].store(true, Ordering::Relaxed);
        assert_defaults("flag set, payload empty");

        // (false, Some(..)) — the mirror state, reachable while a `reset`
        // holds the write lock. The flag is the authority, so the payload's
        // extra word must not be visible.
        *GLOBAL[i].write().expect("lock poisoned") = Some(StopWords::from_iter_of(
            entries.iter().copied().chain(std::iter::once(probe)),
        ));
        MUTATED[i].store(false, Ordering::Relaxed);
        assert_defaults("flag clear, payload present");

        // (true, Some(..)) — an ordinary mutated list.
        MUTATED[i].store(true, Ordering::Relaxed);
        for w in entries {
            assert!(
                contains(lang, w),
                "flag set, payload present: {w:?} vanished"
            );
        }
        assert!(
            contains(lang, probe),
            "flag set, payload present: the addition is not visible"
        );

        reset(lang);
        assert_defaults("after reset");
    }

    /// A panic anywhere near a mutation must not take the whole language's
    /// stop-word list down with it.
    ///
    /// `RwLock` poisons on a panic held across the lock, and every mutator
    /// here takes the *same* lock [`contains`] reads through. Propagating the
    /// poison turned one caller's panic into a panic on every subsequent
    /// `tokenize_and_stem` in the process — the lookup is on the hot path of
    /// every token of every document. This installs a poisoned lock directly
    /// (a panicking closure inside the write guard, caught) and then asks the
    /// same questions a document would.
    #[test]
    fn a_poisoned_lock_does_not_take_the_language_down_with_it() {
        // Italian: no other test in this module mutates it.
        let lang = Language::It;
        let i = lang.index();
        lang.reset();

        // Poison the lock the way a panicking `Into<String>` used to.
        let poisoned = std::panic::catch_unwind(|| {
            let _guard = GLOBAL[i].write().expect("first acquisition succeeds");
            panic!("a caller's own code panicked under the lock");
        });
        assert!(poisoned.is_err(), "the probe did not panic");
        assert!(
            GLOBAL[i].is_poisoned(),
            "the probe did not poison the lock, so this test proves nothing"
        );

        // Every entry of the list is still found, and every operation still
        // answers. Before the fix each of these panicked.
        for entry in lang.defaults() {
            assert!(lang.contains(entry), "{entry:?} vanished with the poison");
        }
        assert_eq!(lang.words().len(), lang.defaults().len());
        lang.add("zzz-poison-probe");
        assert!(lang.contains("zzz-poison-probe"));
        lang.remove("zzz-poison-probe");
        assert!(!lang.contains("zzz-poison-probe"));
        lang.reset();
        assert!(lang.contains("come"), "the list survived the reset");
    }

    #[test]
    fn mutation_is_visible_and_reversible() {
        // Uses a language no other test mutates, so the global stays predictable.
        assert!(!contains(Language::Fa, "zzz-probe"));
        add(Language::Fa, "zzz-probe");
        assert!(contains(Language::Fa, "zzz-probe"));
        remove(Language::Fa, "zzz-probe");
        assert!(!contains(Language::Fa, "zzz-probe"));
        reset(Language::Fa);
    }
}
