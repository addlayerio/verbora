//! Per-language stop-word lists, and the process-global mutability the reference
//! gives them.
//!
//! # What the reference actually does
//!
//! Each the reference `stopwords_xx` exports one array. A stemmer that
//! offers `addStopWord` pushes into that array **in place**; `addStopWords`
//! instead rebinds `.words` to `words.concat(more)`, replacing the array object.
//! Every stemmer of that language reads the live `.words`, so the mutation is
//! process-wide and shared. Only a subset of the stemmers expose mutators at
//! all:
//!
//! | Stemmer | add | addAll | remove | removeAll |
//! |---|:--:|:--:|:--:|:--:|
//! | `PorterStemmer` (en), `LancasterStemmer`, `StemmerId` | yes | yes | yes | yes |
//! | `PorterStemmerNo`, `PorterStemmerSv` | yes | yes | — | — |
//! | `PorterStemmerPt` | — | yes | — | — |
//! | De, Es, Fa, Fr, It, Nl, Ru, Uk, Ja, Carry | — | — | — | — |
//!
//! Calling an absent mutator throws a `TypeError` in the reference; in Rust the
//! method simply does not exist, which is the same contract enforced earlier.
//!
//! English lives in [`verbora_core::stopwords`] — it is shared with the
//! phonetics crate, exactly as the reference `stopwords` is shared in the
//! reference — and this module covers the other thirteen lists in the same
//! style: a static default plus a lazily-created global that is only consulted
//! once something has actually mutated it.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{LazyLock, RwLock};

use verbora_core::StopWords;

use crate::data::stopwords as data;

/// A language whose stop-word list the reference keeps in a module-level array.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Language {
    /// German (`stopwords-iso`'s `de`, 620 words).
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

    /// The default list, in reference source order.
    pub const fn defaults(self) -> &'static [&'static str] {
        match self {
            Self::De => data::DEFAULT_DE,
            Self::Es => data::DEFAULT_ES,
            Self::Fa => data::DEFAULT_FA,
            Self::Fr => data::DEFAULT_FR,
            Self::Id => data::DEFAULT_ID,
            Self::It => data::DEFAULT_IT,
            Self::Ja => data::DEFAULT_JA,
            Self::Nl => data::DEFAULT_NL,
            Self::No => data::DEFAULT_NO,
            Self::Pt => data::DEFAULT_PT,
            Self::Ru => data::DEFAULT_RU,
            Self::Sv => data::DEFAULT_SV,
            Self::Uk => data::DEFAULT_UK,
        }
    }

    /// The default list, deduplicated and sorted, for the lock-free fast path.
    const fn sorted(self) -> &'static [&'static str] {
        match self {
            Self::De => data::SORTED_DE,
            Self::Es => data::SORTED_ES,
            Self::Fa => data::SORTED_FA,
            Self::Fr => data::SORTED_FR,
            Self::Id => data::SORTED_ID,
            Self::It => data::SORTED_IT,
            Self::Ja => data::SORTED_JA,
            Self::Nl => data::SORTED_NL,
            Self::No => data::SORTED_NO,
            Self::Pt => data::SORTED_PT,
            Self::Ru => data::SORTED_RU,
            Self::Sv => data::SORTED_SV,
            Self::Uk => data::SORTED_UK,
        }
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
static MUTATED: [AtomicBool; 13] = [const { AtomicBool::new(false) }; 13];

#[allow(
    clippy::type_complexity,
    reason = "one lazily-built list per language; naming the array adds no clarity"
)]
static GLOBAL: LazyLock<[RwLock<Option<StopWords>>; 13]> =
    LazyLock::new(|| std::array::from_fn(|_| RwLock::new(None)));

fn with_list<R>(lang: Language, f: impl FnOnce(&mut StopWords) -> R) -> R {
    let mut guard = GLOBAL[lang.index()]
        .write()
        .expect("stop-word lock poisoned");
    let list =
        guard.get_or_insert_with(|| StopWords::from_iter_of(lang.defaults().iter().copied()));
    let out = f(list);
    MUTATED[lang.index()].store(true, Ordering::Relaxed);
    out
}

/// Whether `word` is currently a stop word in `lang`.
pub fn contains(lang: Language, word: &str) -> bool {
    if !MUTATED[lang.index()].load(Ordering::Relaxed) {
        return lang.sorted().binary_search(&word).is_ok();
    }
    GLOBAL[lang.index()]
        .read()
        .expect("stop-word lock poisoned")
        .as_ref()
        .is_some_and(|l| l.contains(word))
}

/// Appends one stop word, mirroring `addStopWord`'s in-place `push`.
pub fn add(lang: Language, word: impl Into<String>) {
    with_list(lang, |l| l.add(word));
}

/// Appends several stop words, mirroring `addStopWords`' `concat`.
///
/// The reference rebinds the array here rather than mutating it, which
/// desynchronises anything that captured the old array by value. Nothing in this
/// crate captures by value, so the two forms are observationally identical and
/// both are modelled as an append.
pub fn add_all<I, S>(lang: Language, words: I)
where
    I: IntoIterator<Item = S>,
    S: Into<String>,
{
    with_list(lang, |l| l.add_all(words));
}

/// Removes the **first** occurrence of `word`, mirroring `indexOf` + `splice`.
pub fn remove(lang: Language, word: &str) {
    with_list(lang, |l| l.remove(word));
}

/// Removes the first occurrence of each of `words`.
pub fn remove_all<'a, I>(lang: Language, words: I)
where
    I: IntoIterator<Item = &'a str>,
{
    with_list(lang, |l| l.remove_all(words));
}

/// A snapshot of the current list, in order — what reading `.words` would give.
pub fn words(lang: Language) -> Vec<String> {
    if !MUTATED[lang.index()].load(Ordering::Relaxed) {
        return lang.defaults().iter().map(|s| (*s).to_owned()).collect();
    }
    GLOBAL[lang.index()]
        .read()
        .expect("stop-word lock poisoned")
        .as_ref()
        .map_or_else(
            || lang.defaults().iter().map(|s| (*s).to_owned()).collect(),
            |l| l.words().to_vec(),
        )
}

/// Restores a language's list to its default.
///
/// Has no counterpart in the reference; provided so tests that exercise the
/// global-mutation behaviour can isolate themselves from one another.
pub fn reset(lang: Language) {
    *GLOBAL[lang.index()]
        .write()
        .expect("stop-word lock poisoned") = None;
    MUTATED[lang.index()].store(false, Ordering::Relaxed);
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

    #[test]
    fn sorted_view_agrees_with_source_order() {
        for lang in Language::ALL {
            for w in lang.defaults() {
                assert!(
                    lang.sorted().binary_search(w).is_ok(),
                    "{w:?} missing from the sorted view of {lang:?}"
                );
                assert!(contains(lang, w));
            }
            assert!(lang.sorted().windows(2).all(|p| p[0] < p[1]));
        }
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
