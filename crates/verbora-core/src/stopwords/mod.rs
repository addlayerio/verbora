//! Stop-word lists and the process-global English list.
//!
//! The module is private. Everything below is re-exported from the crate root,
//! and all user-facing prose lives on the public items themselves so that it
//! reaches docs.rs.

mod data;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{LazyLock, PoisonError, RwLock};

use rustc_hash::FxHashSet;

/// A language Verbora ships a stop-word list for.
///
/// # This is the only home of this data
///
/// The sixteen lists exist once in the workspace, in this crate. Until 2026-08
/// thirteen of them existed three times over — once here by way of
/// `verbora-util`, once as `verbora-stemmers`' own copy, and once more as
/// hand-written sorted tables beside that copy — with nothing but a test
/// comparing them. Two copies of data that must agree is how they stop
/// agreeing: Dutch `"je "` shipped with a trailing space in *both* copies, and
/// a repair applied to one would have left the other silently wrong. There is
/// now one copy, and the sorted view membership searches is derived from it
/// rather than typed out beside it.
///
/// # What a stop-word list is, and what it is not
///
/// A stop-word list is a **curatorial artefact**: a set of words some editor
/// judged uninformative for some purpose. There is no algorithm that re-derives
/// one and no standard that fixes its contents, so Verbora does not claim the
/// *membership* of these lists is correct. It specifies the things about them
/// that can be specified, and pins those:
///
/// * the comparison used to test membership (below);
/// * the shape every entry must have to be matchable at all, enumerated entry
///   by entry by this crate's own tests;
/// * the exact size and duplicate count of every list, so that an edit is
///   deliberate.
///
/// Where a list's provenance is known it is named on the variant. Where it is
/// not, that is said rather than implied.
///
/// # The text unit: Unicode scalar sequences, compared exactly
///
/// [`StopWordLanguage::is_stopword`] compares its argument to each entry with
/// `str` equality, which is equality of UTF-8 byte sequences and therefore of
/// Unicode scalar sequences. Nothing else happens to either side. In
/// particular:
///
/// * **No case folding.** `"The"` is not a stop word; `"the"` is. Folding here
///   would need a UAX #21 decision — full or simple, Turkish `i` or not — that
///   belongs to the caller, who knows what produced the token. Every list here
///   is lower-case throughout, so a caller who lower-cases first gets
///   case-insensitive filtering and one who does not keeps the distinction.
/// * **No normalisation.** `"café"` in NFC (`U+00E9`) and in NFD
///   (`U+0065 U+0301`) are different strings and only the NFC spelling is on
///   the French list. Every entry of every list is NFC, and
///   `every_entry_is_already_nfc` enumerates that rather than asserting it, so
///   a caller who normalises input to NFC always agrees with these tables.
/// * **No trimming.** An entry with a stray space could never match, which is
///   why entries are audited for shape.
///
/// # Choosing the Right API
///
/// | Want | Call | Cost | Global state |
/// |---|---|---|---|
/// | Is this word a stop word in language L? | [`StopWordLanguage::is_stopword`] | O(log n) binary search | none — pure |
/// | The whole list, to iterate or copy | [`StopWordLanguage::stopwords`] | free, borrowed | none — pure |
/// | A list you can add to and remove from | [`StopWords::for_language`] | one `String` per entry | none — owned by you |
/// | English membership *including* runtime additions | [`is_global_stopword`] | O(log n), or one read-lock after any mutation | reads the process-global list |
/// | Change English filtering process-wide | [`add_global_stopword`] and neighbours | one write-lock | mutates the process-global list |
///
/// The first three are the right choice for almost every program, and they are
/// the only ones that give the same answer on every call for the life of the
/// process.
///
/// The split matters most for English, where all of them exist.
/// `StopWordLanguage::En` describes the **shipped** list and nothing else:
/// `En.is_stopword` and `En.stopwords` always agree with each other, whatever
/// any other crate has done to the global. The process-global list is the one
/// every English stemmer and the phonetics helpers consult, and it is mutable;
/// reach for it when you want *that* list, and never expect the two to answer
/// alike after a mutation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[non_exhaustive]
pub enum StopWordLanguage {
    /// English (168 words). Also the list the process-global functions start
    /// from.
    En,
    /// German (620 words), from the `stopwords-iso` dataset's German list.
    De,
    /// Spanish (70 words). Provenance unrecorded.
    Es,
    /// Persian (26 words). Provenance unrecorded.
    Fa,
    /// French (168 words). Provenance unrecorded.
    Fr,
    /// Indonesian (809 words). Provenance unrecorded.
    Id,
    /// Italian (290 words). Provenance unrecorded.
    It,
    /// Japanese (109 words). Provenance unrecorded.
    Ja,
    /// Dutch (143 words). Provenance unrecorded.
    Nl,
    /// Norwegian (129 words). Provenance unrecorded.
    No,
    /// Polish (291 words). Provenance unrecorded.
    Pl,
    /// Portuguese (117 words). Provenance unrecorded.
    Pt,
    /// Russian (137 words). Provenance unrecorded.
    Ru,
    /// Swedish (428 words). Provenance unrecorded.
    Sv,
    /// Ukrainian (124 words). Provenance unrecorded.
    Uk,
    /// Chinese (78 words). Provenance unrecorded.
    Zh,
}

/// Every language with a stop-word list, English first and the rest in ISO-639-1
/// code order.
///
/// The order is [`StopWordLanguage::ALL`]'s, and it is the order the derived
/// sorted views are indexed in. It carries no other meaning.
pub static STOPWORD_LANGUAGES: &[StopWordLanguage] = &StopWordLanguage::ALL;

/// Sorted, de-duplicated views of every list, built once on first use.
///
/// Derived from [`StopWordLanguage::stopwords`] rather than written out beside
/// it: a computed table cannot drift from its source, and a hand-typed one
/// demonstrably can — this workspace carried thirty such tables between two
/// crates, and the only thing keeping them honest was a test.
///
/// All sixteen are built together rather than one lazy cell each. That does
/// slightly more work than a program filtering a single language needs —
/// sorting roughly 3,700 `&'static str` pointers, once, on the first membership
/// test of the process — in exchange for one `LazyLock` and one atomic check
/// instead of sixteen. No allocation is repeated afterwards, and the
/// alternative was sixteen named statics that cannot be written as an array
/// because each closure would have its own type.
static SORTED: LazyLock<[Box<[&'static str]>; 16]> = LazyLock::new(|| {
    std::array::from_fn(|i| {
        let mut words = StopWordLanguage::ALL[i].stopwords().to_vec();
        words.sort_unstable();
        words.dedup();
        words.into_boxed_slice()
    })
});

impl StopWordLanguage {
    /// Every language this crate carries a list for.
    pub const ALL: [Self; 16] = [
        Self::En,
        Self::De,
        Self::Es,
        Self::Fa,
        Self::Fr,
        Self::Id,
        Self::It,
        Self::Ja,
        Self::Nl,
        Self::No,
        Self::Pl,
        Self::Pt,
        Self::Ru,
        Self::Sv,
        Self::Uk,
        Self::Zh,
    ];

    /// The ISO 639-1 code, lower-case.
    pub const fn code(self) -> &'static str {
        match self {
            Self::En => "en",
            Self::De => "de",
            Self::Es => "es",
            Self::Fa => "fa",
            Self::Fr => "fr",
            Self::Id => "id",
            Self::It => "it",
            Self::Ja => "ja",
            Self::Nl => "nl",
            Self::No => "no",
            Self::Pl => "pl",
            Self::Pt => "pt",
            Self::Ru => "ru",
            Self::Sv => "sv",
            Self::Uk => "uk",
            Self::Zh => "zh",
        }
    }

    /// Looks a language up by ISO 639-1 code, **case-sensitively**.
    ///
    /// `from_code("en")` is `Some`, `from_code("EN")` is `None`. Callers
    /// holding a code of unknown casing lower-case it themselves — the same
    /// division of labour [`Self::is_stopword`] applies to words.
    ///
    /// ```
    /// use verbora_core::StopWordLanguage;
    ///
    /// assert_eq!(StopWordLanguage::from_code("sv"), Some(StopWordLanguage::Sv));
    /// assert_eq!(StopWordLanguage::from_code("SV"), None);
    /// assert_eq!(StopWordLanguage::from_code("xx"), None);
    /// ```
    pub fn from_code(code: &str) -> Option<Self> {
        Self::ALL.iter().copied().find(|l| l.code() == code)
    }

    /// This language's slot in [`SORTED`].
    ///
    /// A `match` rather than a scan of [`Self::ALL`]: this runs on every
    /// membership test, and the compiler turns it into an immediate. The
    /// `ordinal_agrees_with_the_language_table` test checks the two against
    /// each other, so the hand-written arm cannot drift from the array it
    /// indexes.
    const fn ordinal(self) -> usize {
        match self {
            Self::En => 0,
            Self::De => 1,
            Self::Es => 2,
            Self::Fa => 3,
            Self::Fr => 4,
            Self::Id => 5,
            Self::It => 6,
            Self::Ja => 7,
            Self::Nl => 8,
            Self::No => 9,
            Self::Pl => 10,
            Self::Pt => 11,
            Self::Ru => 12,
            Self::Sv => 13,
            Self::Uk => 14,
            Self::Zh => 15,
        }
    }

    /// The shipped list, in source order, duplicates included.
    ///
    /// Both properties are observable because this borrows the backing slice.
    /// Source order carries no meaning beyond being the order the data was
    /// compiled in — it is not a frequency ranking. Several lists repeat an
    /// entry (German 1, Spanish 2, Dutch 1, Norwegian 2, Polish 1, Russian 8,
    /// Ukrainian 6); a repeat cannot affect membership, which searches a
    /// de-duplicated view, and the exact counts are pinned by test.
    ///
    /// For [`Self::En`] this is the shipped default, which is **not** affected
    /// by [`add_global_stopword`]. Use [`global_stopwords`] for a snapshot that
    /// is.
    pub const fn stopwords(self) -> &'static [&'static str] {
        match self {
            Self::En => data::EN,
            Self::De => data::DE,
            Self::Es => data::ES,
            Self::Fa => data::FA,
            Self::Fr => data::FR,
            Self::Id => data::ID,
            Self::It => data::IT,
            Self::Ja => data::JA,
            Self::Nl => data::NL,
            Self::No => data::NO,
            Self::Pl => data::PL,
            Self::Pt => data::PT,
            Self::Ru => data::RU,
            Self::Sv => data::SV,
            Self::Uk => data::UK,
            Self::Zh => data::ZH,
        }
    }

    /// The list sorted by UTF-8 bytes and de-duplicated.
    fn sorted(self) -> &'static [&'static str] {
        &SORTED[self.ordinal()]
    }

    /// Whether `word` is on this language's shipped stop-word list.
    ///
    /// Compares Unicode scalar sequences exactly: no case folding, no
    /// normalisation, no trimming. See the type-level note for why each of
    /// those is the caller's choice rather than this function's.
    ///
    /// A pure function of the shipped data for **every** language, English
    /// included: it never consults the process-global list, so two calls with
    /// the same argument always agree. `""` is not a stop word in any language.
    ///
    /// O(log n) by binary search over a de-duplicated view built on first use.
    ///
    /// ```
    /// use verbora_core::StopWordLanguage;
    ///
    /// assert!(StopWordLanguage::En.is_stopword("the"));
    /// assert!(!StopWordLanguage::En.is_stopword("The"));
    /// assert!(StopWordLanguage::De.is_stopword("ei"));
    /// assert!(!StopWordLanguage::Es.is_stopword(""));
    /// ```
    pub fn is_stopword(self, word: &str) -> bool {
        self.sorted().binary_search(&word).is_ok()
    }
}

/// An ordered, owned stop-word list with O(1) membership testing.
///
/// Insertion order is preserved and observable through [`Self::words`], while
/// [`Self::contains`] goes through a hash set. Nothing here is shared: a
/// `StopWords` is a plain value, and mutating it affects only the caller who
/// owns it. That is the difference from the process-global functions below,
/// and it is why every consumer in this workspace that filters against a list
/// takes one of these by reference rather than reading a global.
///
/// # Choosing the Right API
///
/// | Want | Call | Notes |
/// |---|---|---|
/// | The shipped list for a language, as an owned value | [`Self::for_language`] | one `String` per entry, duplicates kept |
/// | An empty list, to add to | [`Self::new`] | allocates nothing until the first `add` |
/// | A list from words you already have | [`Self::from_iter_of`] | takes anything `Into<String>`, including `&str` |
/// | The same, at the end of an iterator chain | `.collect()` | `FromIterator` is implemented for the same element types |
///
/// [`Self::from_iter_of`] and `.collect()` build the same value; the named
/// constructor exists because `StopWords::from_iter_of(["the", "a"])` needs no
/// turbofish and no intermediate binding, which is how it is almost always
/// called.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StopWords {
    ordered: Vec<String>,
    lookup: FxHashSet<String>,
}

impl StopWords {
    /// Creates an empty list.
    ///
    /// Every word is a stop word in no list, so an empty `StopWords` passed to
    /// a filtering API keeps every token.
    pub fn new() -> Self {
        Self::default()
    }

    /// Creates an owned copy of a language's shipped list.
    ///
    /// The copy is independent: adding to it does not change
    /// [`StopWordLanguage::stopwords`], the process-global list, or anyone
    /// else's copy.
    ///
    /// ```
    /// use verbora_core::{StopWordLanguage, StopWords};
    ///
    /// let mut stops = StopWords::for_language(StopWordLanguage::En);
    /// assert!(stops.contains("the"));
    /// stops.add("verbora");
    /// assert!(stops.contains("verbora"));
    /// assert!(!StopWordLanguage::En.is_stopword("verbora"));
    /// ```
    pub fn for_language(language: StopWordLanguage) -> Self {
        Self::from_iter_of(language.stopwords().iter().copied())
    }

    /// Builds a list from any iterator of string-likes.
    ///
    /// Order is preserved and duplicates are kept in [`Self::words`], because
    /// that slice is observable; the lookup set de-duplicates internally, so a
    /// duplicate cannot affect [`Self::contains`].
    pub fn from_iter_of<I, S>(words: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        let ordered: Vec<String> = words.into_iter().map(Into::into).collect();
        let lookup = ordered.iter().cloned().collect();
        Self { ordered, lookup }
    }

    /// Returns whether `word` is in the list.
    ///
    /// Exact comparison of Unicode scalar sequences, like
    /// [`StopWordLanguage::is_stopword`]: no case folding, no normalisation, no
    /// trimming.
    pub fn contains(&self, word: &str) -> bool {
        self.lookup.contains(word)
    }

    /// Returns the words in insertion order, duplicates included.
    pub fn words(&self) -> &[String] {
        &self.ordered
    }

    /// Returns the number of words, counting duplicates.
    pub fn len(&self) -> usize {
        self.ordered.len()
    }

    /// Returns whether the list is empty.
    pub fn is_empty(&self) -> bool {
        self.ordered.is_empty()
    }

    /// Appends a stop word.
    ///
    /// Appending is unconditional, so adding a word twice puts it in
    /// [`Self::words`] twice and [`Self::remove`] then has to be called twice
    /// to take it out. [`Self::contains`] is unaffected either way.
    pub fn add(&mut self, word: impl Into<String>) {
        let word = word.into();
        self.lookup.insert(word.clone());
        self.ordered.push(word);
    }

    /// Appends several stop words.
    pub fn add_all<I, S>(&mut self, words: I)
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        for w in words {
            self.add(w);
        }
    }

    /// Removes the **first** occurrence of `word`, returning whether one was
    /// found.
    ///
    /// A word added twice needs removing twice; the return value is what tells
    /// a caller which of those happened, rather than leaving a call that did
    /// nothing indistinguishable from one that did.
    pub fn remove(&mut self, word: &str) -> bool {
        let Some(idx) = self.ordered.iter().position(|w| w == word) else {
            return false;
        };
        self.ordered.remove(idx);
        // Only drop from the lookup set once no occurrences remain.
        if !self.ordered.iter().any(|w| w == word) {
            self.lookup.remove(word);
        }
        true
    }

    /// Removes the first occurrence of each of `words`, returning how many were
    /// found.
    pub fn remove_all<'a, I>(&mut self, words: I) -> usize
    where
        I: IntoIterator<Item = &'a str>,
    {
        words.into_iter().filter(|w| self.remove(w)).count()
    }
}

impl<S: Into<String>> FromIterator<S> for StopWords {
    fn from_iter<I: IntoIterator<Item = S>>(iter: I) -> Self {
        Self::from_iter_of(iter)
    }
}

// ---------------------------------------------------------------------------
// The process-global English list
// ---------------------------------------------------------------------------

/// Whether the process-global list has ever been mutated.
///
/// `Relaxed` is sufficient: this flag only selects between two lookup
/// strategies that are both correct, and the `RwLock` provides the actual
/// synchronisation for the mutated case.
///
/// # Why every writer publishes it under the lock
///
/// The flag and [`GLOBAL`] are two words, and a writer that released the lock
/// before touching the flag would let two writers interleave into a state
/// neither of them asked for. Concretely, with the flag stored after the guard
/// had been dropped:
///
/// ```text
/// thread A: reset  — installs the shipped list, releases the lock
/// thread B: add    — appends "verbora", releases the lock, sets the flag
/// thread A:        — clears the flag
/// ```
///
/// leaves `false` next to a list that does contain `"verbora"`, so
/// [`is_global_stopword`] takes the lock-free path forever and the addition is
/// invisible to every reader — not stale, unreachable. Storing the flag while
/// the write guard is still alive orders the two mutations against each other,
/// so the loser of the race is whichever ran first, and its whole effect is
/// lost rather than half of it.
///
/// Readers still sample the pair unsynchronised, which is fine here: [`GLOBAL`]
/// is never emptied — [`reset_global_stopwords`] *replaces* it with the shipped
/// English list rather than clearing it — so the locked path always has a whole
/// list to answer from, whichever instant a reader catches.
static GLOBAL_MUTATED: AtomicBool = AtomicBool::new(false);

/// The process-global English stop-word list.
static GLOBAL: LazyLock<RwLock<StopWords>> =
    LazyLock::new(|| RwLock::new(StopWords::for_language(StopWordLanguage::En)));

/// Tests membership against the process-global English stop-word list.
///
/// This is **not** [`StopWordLanguage::En::is_stopword`](StopWordLanguage::is_stopword):
/// it answers about the list as the process has left it, additions and removals
/// included, so two calls with the same argument can disagree if something
/// mutated the list in between. Reach for it when you want that; reach for
/// `StopWordLanguage::En.is_stopword` when you want a pure function of the
/// shipped data.
///
/// Fast path — no mutation has ever occurred — is a binary search over a static
/// sorted slice, with no locking and no allocation.
///
/// ```
/// use verbora_core::{is_global_stopword, reset_global_stopwords};
///
/// reset_global_stopwords();
/// assert!(is_global_stopword("the"));
/// assert!(!is_global_stopword("The"));
/// ```
pub fn is_global_stopword(word: &str) -> bool {
    if !GLOBAL_MUTATED.load(Ordering::Relaxed) {
        return StopWordLanguage::En.is_stopword(word);
    }
    read_global().contains(word)
}

/// Adds a stop word to the process-global English list.
///
/// This changes the answer every reader of the global list gets, for the rest
/// of the process — including every English stemmer and the phonetics helpers
/// that consult it. A library that calls this is changing its caller's
/// behaviour; [`reset_global_stopwords`] is the way back.
pub fn add_global_stopword(word: impl Into<String>) {
    // The `Into<String>` conversion is the caller's code. Running it under the
    // write lock would let a panicking implementation poison the lock every
    // membership test in the process goes through, so it happens first.
    let word = word.into();
    with_global(|list| list.add(word));
}

/// Adds several stop words to the process-global English list.
pub fn add_global_stopwords<I, S>(words: I)
where
    I: IntoIterator<Item = S>,
    S: Into<String>,
{
    let words: Vec<String> = words.into_iter().map(Into::into).collect();
    with_global(|list| list.add_all(words));
}

/// Removes the first occurrence of `word` from the process-global English list,
/// returning whether one was found.
pub fn remove_global_stopword(word: &str) -> bool {
    with_global(|list| list.remove(word))
}

/// Removes the first occurrence of each of `words` from the process-global
/// English list, returning how many were found.
pub fn remove_global_stopwords<'a, I>(words: I) -> usize
where
    I: IntoIterator<Item = &'a str>,
{
    let words: Vec<&'a str> = words.into_iter().collect();
    with_global(|list| list.remove_all(words))
}

/// Applies one mutation to the process-global list and announces it.
///
/// Every mutator goes through here so that the announcement cannot drift from
/// the mutation: the flag is stored while the write guard is still alive, which
/// is what orders concurrent writers against one another. [`GLOBAL_MUTATED`]
/// has the interleaving that motivates it.
fn with_global<R>(f: impl FnOnce(&mut StopWords) -> R) -> R {
    let mut guard = GLOBAL.write().unwrap_or_else(PoisonError::into_inner);
    let out = f(&mut guard);
    GLOBAL_MUTATED.store(true, Ordering::Relaxed);
    drop(guard);
    out
}

/// Reads the process-global list.
///
/// # Poisoning is recovered from, not propagated
///
/// A poisoned lock would make **every** later stop-word operation in the
/// process panic — including [`is_global_stopword`], which runs on every token
/// of every document a stemmer filters — so a single panic anywhere near a
/// mutation would turn every subsequent `tokenize_and_stem` call into a panic.
/// That is a far worse failure than the one it reports, and the state it guards
/// does not need it: the payload is a list of strings whose every intermediate
/// value is a valid list, so there is no invariant a partially-applied mutation
/// could have broken. Callers therefore see the list as it stands.
///
/// The mutators above also do all of their *caller-supplied* work before taking
/// the lock — a user `Into<String>` or `IntoIterator` implementation that
/// panics is the only realistic way to poison this lock — so in practice the
/// recovery is a second line of defence rather than the first.
fn read_global() -> std::sync::RwLockReadGuard<'static, StopWords> {
    GLOBAL.read().unwrap_or_else(PoisonError::into_inner)
}

/// Returns a snapshot of the process-global English list, in order.
pub fn global_stopwords() -> Vec<String> {
    if !GLOBAL_MUTATED.load(Ordering::Relaxed) {
        return StopWordLanguage::En
            .stopwords()
            .iter()
            .map(|s| (*s).to_owned())
            .collect();
    }
    read_global().words().to_vec()
}

/// Restores the process-global English list to the shipped list.
///
/// There is no other way back: every mutator above is process-wide, so a test
/// that adds a stop word changes the answer every other caller in the process
/// gets until this is called.
///
/// Two details of how it does that are load-bearing rather than cosmetic.
///
/// The list is *replaced* with the shipped one rather than emptied, so the
/// locked path always has a whole list to answer from: a reader that catches
/// this call mid-flight sees either the list as it was or the shipped list —
/// never a list that is missing. `verbora-stemmers` shipped the other shape for
/// its own per-language lists, and a `contains` racing a `reset` there reported
/// the whole language as containing no stop words at all.
///
/// The has-ever-been-mutated flag described on [`is_global_stopword`] is
/// cleared while the write guard is still alive, which is what orders this call
/// against a concurrent [`add_global_stopword`]. Were it cleared after the
/// guard were released, an `add` could interleave between the two — appending
/// its word, releasing the lock and setting the flag, only for this function's
/// clear to land afterwards. That leaves the flag reading "never mutated"
/// beside a list that *was* mutated, so [`is_global_stopword`] takes the
/// lock-free path over the shipped list forever and the addition is unreachable
/// rather than merely stale.
pub fn reset_global_stopwords() {
    let mut guard = GLOBAL.write().unwrap_or_else(PoisonError::into_inner);
    GLOBAL_MUTATED.store(false, Ordering::Relaxed);
    *guard = StopWords::for_language(StopWordLanguage::En);
    drop(guard);
}

#[cfg(test)]
mod tests;
