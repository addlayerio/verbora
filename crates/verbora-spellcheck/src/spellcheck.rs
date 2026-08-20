//! [`Spellcheck`]: corrections for a misspelled word, drawn from a corpus.

use std::fmt;
use std::hash::Hasher;
use std::sync::OnceLock;

use rustc_hash::{FxHashMap, FxHasher};
use verbora_distance::damerau_levenshtein;

use crate::deletions::{distinct_deletions, to_scalars};

/// The largest `max_distance` the lazily built deletion index answers.
///
/// Two, because a symmetric-delete index costs `scalar length choose depth` per
/// stored word to build, and depth 3 turns that into a structure larger than
/// the corpus it indexes. Beyond this the query falls back to a scan; see
/// [`Spellcheck::corrections`]'s "Cost" section.
const NEAR_DISTANCE: u32 = 2;

/// The largest number of **distinct** words one [`Spellcheck`] can hold.
///
/// Every entry is addressed by a `u32` — in the word index, in the deletion
/// buckets, and in the candidate lists — because a 32-bit id halves the size of
/// those structures against a `usize` one. That choice buys memory at the price
/// of a ceiling, and this constant is the ceiling, named rather than implied.
///
/// Repeats do not count: a corpus of a trillion occurrences of ten words holds
/// ten distinct words. Reaching the limit therefore needs 4.29 billion *distinct*
/// strings resident at once, which measures at roughly 156 bytes each — about
/// 620 GiB before any of them is corrected. [`Spellcheck::try_new`] reports it
/// rather than panicking, on the principle `verbora_util`'s graph builder states
/// for the identical situation: a capacity a caller controls belongs in a
/// `Result`, not in a panic.
pub const MAX_DISTINCT_WORDS: u32 = u32::MAX;

/// Returned by [`Spellcheck::try_new`] when a corpus holds more distinct words
/// than one [`Spellcheck`] can address.
///
/// Not a soft limit: entry ids are `u32`, so the 4,294,967,296th distinct word
/// has no id to be given. Split the corpus, or deduplicate it further, if you
/// somehow have one this large — see [`MAX_DISTINCT_WORDS`] for what that costs
/// in memory.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct CorpusTooLarge {
    /// The largest number of distinct words that would have been accepted.
    pub limit: u32,
}

impl fmt::Display for CorpusTooLarge {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "corpus holds more than {} distinct words, which is all a Spellcheck can address",
            self.limit
        )
    }
}

impl std::error::Error for CorpusTooLarge {}

/// One suggested correction.
///
/// `word` borrows from the [`Spellcheck`], so a correction costs no allocation
/// at all; `distance` and `frequency` are the two numbers the ordering is built
/// from, returned rather than hidden so a caller can re-rank, filter or display
/// them without asking twice.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Correction<'a> {
    /// The corpus word.
    pub word: &'a str,
    /// Its exact edit distance from the query, under this crate's metric —
    /// [`verbora_distance::damerau_levenshtein`], counted in Unicode scalars.
    pub distance: u32,
    /// How many times `word` occurs in the corpus the [`Spellcheck`] was built
    /// from. Always at least 1.
    pub frequency: u32,
}

/// Spelling correction over a corpus, after
/// [Norvig](https://norvig.com/spell-correct.html).
///
/// The word list is a *corpus*, not a dictionary: a word's number of
/// occurrences is its probability, and corrections come back most-probable
/// first. Feeding it real text works far better than feeding it
/// `/usr/share/dict/words`, where every word has frequency 1 and only edit
/// distance separates the candidates.
///
/// ```
/// use verbora_spellcheck::Spellcheck;
///
/// // Repeats are frequencies, and frequency breaks ties within a distance.
/// let sc = Spellcheck::new(["the", "the", "the", "he", "he", "she", "th"]);
///
/// assert!(sc.is_correct("the"));
/// assert_eq!(sc.correction_words("he", 1), ["he", "the", "she"]);
/// ```
///
/// # The contract
///
/// `corrections(query, k)` returns **every corpus word whose edit distance from
/// `query` is at most `k`, and nothing else**. The metric is
/// [`verbora_distance::damerau_levenshtein`] — unrestricted
/// Damerau–Levenshtein, unit cost, counted in **Unicode scalar values**
/// (`docs/design/distance-contract.md` §2).
///
/// Defining the answer by a metric rather than by an enumeration procedure is
/// what makes it checkable: the result of `corrections` is, by definition, the
/// same set a brute-force scan over the whole corpus would produce, and this
/// crate's tests assert exactly that over every entry of a real word list. It
/// also settles the questions an enumeration-defined answer leaves open:
///
/// * **Transposition costs one edit.** `"hte"` corrects to `"the"` at `k = 1`.
///   That is why the metric is Damerau–Levenshtein and not plain Levenshtein:
///   transposed adjacent letters are the commonest typo class there is.
/// * **There is no alphabet.** A correction is not restricted to words spelled
///   with some fixed letter set, so `"cafe"` corrects to `"café"` and `"Мсква"`
///   to `"Москва"` at `k = 1`, in any script, without configuration.
/// * **An astral scalar is one unit.** Deleting an emoji is one edit, not two.
/// * **Case is not special.** `is_correct` is exact, and a case difference is
///   one substitution like any other — so `Spellcheck::new(["Cat"])` does
///   correct `"cat"` at `k = 1`.
///
/// # Order
///
/// Ascending edit distance first — a word reachable in `k` edits is treated as
/// more probable than any word needing `k + 1`, however frequent — then
/// descending corpus frequency, then ascending `<str as Ord>` on the word
/// itself. The last key is what makes the order **total**: no two corrections
/// can tie on all three, so the sequence is fully determined by the corpus and
/// the query, with nothing left to the enumeration order of any internal
/// structure.
///
/// ```
/// use verbora_spellcheck::Spellcheck;
///
/// // "zz" is by far the most frequent word, but it is two edits from "aa"
/// // while "a" and "az" are one, and distance dominates frequency absolutely.
/// let sc = Spellcheck::new(["zz", "zz", "zz", "zz", "az", "a", "a"]);
/// assert_eq!(sc.correction_words("aa", 2), ["a", "az", "zz"]);
/// ```
///
/// # Choosing the Right API
///
/// | API | Returns | Reach for it when |
/// |---|---|---|
/// | [`Spellcheck::is_correct`] | `bool` | you only need to know whether a word is in the corpus. One hash lookup; never generates a candidate. |
/// | [`Spellcheck::best_correction`] | `Option<Correction>` | you want the single best suggestion. Skips the sort — it scans candidates for the minimum. |
/// | [`Spellcheck::corrections`] | `Vec<Correction>` | you want a ranked list, with the distance and frequency behind the ranking. **The general case.** |
/// | [`Spellcheck::correction_words`] | `Vec<String>` | you want the ranked words and nothing else — one `String` per suggestion. |
/// | [`Spellcheck::par_corrections_batch`] | `Vec<Vec<Correction>>` | you are correcting a document-sized batch against one corpus (`parallel` feature). |
///
/// Decision tree:
///
/// ```text
/// is the word simply in the corpus?  ── yes ── is_correct
/// need one suggestion?               ── yes ── best_correction
/// need the ranking numbers?          ── yes ── corrections
/// need owned words only?             ── yes ── correction_words
/// correcting many words at once?     ── yes ── par_corrections_batch
/// ```
///
/// `corrections` is the primitive; `best_correction` and `correction_words`
/// are built on the same retrieval and differ only in what they keep.
/// `is_correct` is not a special case of any of them — it does no candidate
/// work at all, so use it when that is the whole question.
#[derive(Debug)]
pub struct Spellcheck {
    /// Distinct words, in first-occurrence order.
    words: Vec<Box<str>>,
    /// `frequencies[i]` is the number of occurrences of `words[i]`.
    frequencies: Vec<u32>,
    index: FxHashMap<Box<str>, u32>,
    /// Deletion-sequence hash → indices of the entries that share it, built on
    /// the first near-distance query so construction cost is unchanged for
    /// callers who never make one.
    near: OnceLock<FxHashMap<u64, Vec<u32>>>,
}

impl Spellcheck {
    /// Builds a spellchecker from a corpus.
    ///
    /// Repeats are meaningful: they are the frequencies. The list is walked
    /// once, in order, so the first occurrence of each word fixes its position
    /// in [`Spellcheck::frequencies`].
    ///
    /// A word occurring more than `u32::MAX` times saturates there rather than
    /// wrapping or panicking; at that point the ordering it participates in is
    /// no longer meaningful anyway, and a corpus that large has other problems.
    ///
    /// ```
    /// use verbora_spellcheck::Spellcheck;
    ///
    /// let sc = Spellcheck::new(["the", "cat", "the"]);
    /// assert_eq!(sc.frequency("the"), Some(2));
    /// assert_eq!(sc.frequency("dog"), None);
    /// ```
    ///
    /// # Panics
    ///
    /// On a corpus of more than [`MAX_DISTINCT_WORDS`] **distinct** words —
    /// roughly 620 GiB of them, so not on any corpus that fits in a normal
    /// machine. Use [`Spellcheck::try_new`] to receive that as a
    /// [`CorpusTooLarge`] instead; this constructor exists because handling an
    /// error no realistic corpus can produce is noise at most call sites.
    #[must_use]
    pub fn new<I, S>(wordlist: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        Self::build(wordlist, MAX_DISTINCT_WORDS).unwrap_or_else(|e| panic!("Spellcheck::new: {e}"))
    }

    /// Builds a spellchecker from a corpus, reporting the one way it can fail.
    ///
    /// Identical to [`Spellcheck::new`] in every other respect.
    ///
    /// # Errors
    ///
    /// [`CorpusTooLarge`] when the corpus holds more than
    /// [`MAX_DISTINCT_WORDS`] distinct words. Repeats are free: they raise a
    /// frequency and consume no id, so only the number of *distinct* words is
    /// bounded.
    ///
    /// ```
    /// use verbora_spellcheck::Spellcheck;
    ///
    /// let sc = Spellcheck::try_new(["the", "cat", "the"]).expect("a small corpus");
    /// assert_eq!(sc.frequency("the"), Some(2));
    /// ```
    pub fn try_new<I, S>(wordlist: I) -> Result<Self, CorpusTooLarge>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        Self::build(wordlist, MAX_DISTINCT_WORDS)
    }

    /// The shared body of [`Spellcheck::new`] and [`Spellcheck::try_new`],
    /// with the ceiling as a parameter so the boundary is testable at a small
    /// value rather than only at 4.29 billion distinct words — which needs
    /// hundreds of gigabytes of RAM and so cannot be reached in a test suite.
    ///
    /// The id counter is a `u32` and is compared against `limit` *before* it is
    /// handed out, so an id that does not fit is not merely rejected: it is
    /// never formed. That is why there is no fallible narrowing anywhere on
    /// this path.
    fn build<I, S>(wordlist: I, limit: u32) -> Result<Self, CorpusTooLarge>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let mut words: Vec<Box<str>> = Vec::new();
        let mut frequencies: Vec<u32> = Vec::new();
        let mut index: FxHashMap<Box<str>, u32> = FxHashMap::default();
        let mut next_id: u32 = 0;

        for word in wordlist {
            let word = word.as_ref();
            if let Some(&i) = index.get(word) {
                frequencies[i as usize] = frequencies[i as usize].saturating_add(1);
            } else {
                if next_id == limit {
                    return Err(CorpusTooLarge { limit });
                }
                index.insert(word.into(), next_id);
                words.push(word.into());
                frequencies.push(1);
                next_id += 1;
            }
        }

        Ok(Self {
            words,
            frequencies,
            index,
            near: OnceLock::new(),
        })
    }

    /// The number of distinct words in the corpus.
    #[must_use]
    pub fn len(&self) -> usize {
        self.words.len()
    }

    /// Whether the corpus holds no words at all.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.words.is_empty()
    }

    /// Whether `word` is in the corpus, exactly and case-sensitively.
    ///
    /// One hash lookup — this never generates a candidate or computes a
    /// distance.
    ///
    /// ```
    /// use verbora_spellcheck::Spellcheck;
    ///
    /// let sc = Spellcheck::new(["", "cat"]);
    /// assert!(sc.is_correct("cat"));
    /// assert!(!sc.is_correct("Cat"));
    /// // An empty string in the corpus is a word like any other.
    /// assert!(sc.is_correct(""));
    /// ```
    #[must_use]
    pub fn is_correct(&self, word: &str) -> bool {
        self.index.contains_key(word)
    }

    /// How many times `word` occurs in the corpus, or `None` when it does not
    /// occur at all.
    ///
    /// A frequency is a count: it is never zero for a word that is present, and
    /// never a floating-point value that could be `NaN`.
    #[must_use]
    pub fn frequency(&self, word: &str) -> Option<u32> {
        self.index.get(word).map(|&i| self.frequencies[i as usize])
    }

    /// Every distinct word and its frequency, in **first-occurrence order** —
    /// the order the corpus introduced them.
    ///
    /// ```
    /// use verbora_spellcheck::Spellcheck;
    ///
    /// let sc = Spellcheck::new(["b", "10", "2", "a", "b"]);
    /// assert_eq!(
    ///     sc.frequencies().collect::<Vec<_>>(),
    ///     [("b", 2), ("10", 1), ("2", 1), ("a", 1)]
    /// );
    /// ```
    pub fn frequencies(&self) -> impl Iterator<Item = (&str, u32)> {
        self.words
            .iter()
            .zip(&self.frequencies)
            .map(|(w, &f)| (&**w, f))
    }

    /// Every corpus word within `max_distance` edits of `word`, ordered as the
    /// type-level documentation describes: ascending distance, then descending
    /// frequency, then ascending word.
    ///
    /// `max_distance` means exactly what it says: `0` asks for zero edits, so
    /// the answer is the word itself when it is in the corpus and nothing
    /// otherwise.
    ///
    /// ```
    /// use verbora_spellcheck::Spellcheck;
    ///
    /// let sc = Spellcheck::new(["something", "somewhat"]);
    /// let got = sc.corrections("smething", 1);
    /// assert_eq!(got.len(), 1);
    /// assert_eq!(got[0].word, "something");
    /// assert_eq!(got[0].distance, 1);
    ///
    /// assert!(sc.corrections("smething", 0).is_empty());
    /// assert_eq!(sc.corrections("something", 0)[0].word, "something");
    /// ```
    ///
    /// # Cost
    ///
    /// At `max_distance <= 2` — the overwhelmingly common case, and the range
    /// Norvig's own figures put 80–95% of real misspellings in — candidates
    /// come from a symmetric-delete index built lazily on the first such call,
    /// so only the handful of corpus words sharing a deletion sequence with
    /// `word` have their distance computed. Above 2 the query falls back to a
    /// scan of the corpus, skipping any word whose scalar length already
    /// differs by more than `max_distance` (a valid lower bound on the metric)
    /// and computing the distance for the rest. Neither path has been
    /// benchmarked since this crate's contract changed, so no speed figure is
    /// quoted here; what is stated is the work each path does.
    ///
    /// # Allocation
    ///
    /// One `Vec` for the candidate indices, one for the result. A
    /// [`Correction`] borrows its word from `self`, so no string is copied.
    #[must_use]
    pub fn corrections(&self, word: &str, max_distance: u32) -> Vec<Correction<'_>> {
        let mut out = Vec::new();
        self.for_each_match(word, max_distance, |c| out.push(c));
        out.sort_unstable_by(|a, b| {
            a.distance
                .cmp(&b.distance)
                .then(b.frequency.cmp(&a.frequency))
                .then_with(|| a.word.cmp(b.word))
        });
        out
    }

    /// [`Spellcheck::corrections`] reduced to the words, owned.
    ///
    /// The convenience shape for callers that only display or store the
    /// suggestions. It costs one `String` per suggestion; `corrections`
    /// costs none.
    ///
    /// ```
    /// use verbora_spellcheck::Spellcheck;
    ///
    /// let sc = Spellcheck::new(["the", "the", "the", "he", "he", "she", "th"]);
    /// assert_eq!(sc.correction_words("th", 1), ["th", "the"]);
    /// ```
    #[must_use]
    pub fn correction_words(&self, word: &str, max_distance: u32) -> Vec<String> {
        self.corrections(word, max_distance)
            .into_iter()
            .map(|c| c.word.to_owned())
            .collect()
    }

    /// The single best correction — the first element
    /// [`Spellcheck::corrections`] would return — without sorting the rest.
    ///
    /// ```
    /// use verbora_spellcheck::Spellcheck;
    ///
    /// let sc = Spellcheck::new(["something", "somewhat"]);
    /// assert_eq!(sc.best_correction("smething", 1).map(|c| c.word), Some("something"));
    /// assert_eq!(sc.best_correction("xyzzy", 1), None);
    /// ```
    #[must_use]
    pub fn best_correction(&self, word: &str, max_distance: u32) -> Option<Correction<'_>> {
        let mut best: Option<Correction<'_>> = None;
        self.for_each_match(word, max_distance, |c| {
            let better = match best {
                None => true,
                Some(b) => {
                    (c.distance, std::cmp::Reverse(c.frequency), c.word)
                        < (b.distance, std::cmp::Reverse(b.frequency), b.word)
                }
            };
            if better {
                best = Some(c);
            }
        });
        best
    }

    /// [`Spellcheck::corrections`], fanned out across a `rayon` thread pool.
    /// Requires the `parallel` feature.
    ///
    /// # Why this exists
    ///
    /// [`Spellcheck`] is immutable after construction and holds nothing but
    /// owned data; its only interior mutability is the `OnceLock` guarding the
    /// lazily built deletion index, which is itself `Send + Sync` — so
    /// [`Spellcheck`] is `Send + Sync` automatically, and looking up many words
    /// against it has zero coordination cost between lookups after the one-time
    /// index build. This function is exactly
    /// `words.par_iter().map(|w| self.corrections(w, max_distance)).collect()`
    /// — a thin fan-out over the existing sequential primitive, not a second
    /// implementation of it. If you need a different shape in parallel
    /// (per-word `max_distance`, say), apply the same `par_iter().map(...)`
    /// pattern at your own call site.
    ///
    /// # When to reach for it
    ///
    /// When correcting a *batch* against one corpus is itself the unit of work
    /// — spell-checking a document, or a column of free-text input — and the
    /// batch runs to dozens of words or more. A `rayon` task costs on the order
    /// of a microsecond to schedule; a short batch at `max_distance = 1` may
    /// not clear that, so measure before preferring it to a plain
    /// `words.iter().map(...)` loop there. Do not reach for it when the caller
    /// already parallelises at a coarser grain (one request per thread, say),
    /// where a second layer of parallelism oversubscribes the CPU.
    ///
    /// # Order and errors
    ///
    /// Output order matches input order — `results[i]` is
    /// `self.corrections(words[i], max_distance)` — via `rayon`'s
    /// order-preserving `map` + `collect`. There is no error case: a word with
    /// no corrections yields an empty `Vec`, exactly as in the sequential path.
    ///
    /// ```
    /// use verbora_spellcheck::Spellcheck;
    ///
    /// let sc = Spellcheck::new(["the", "the", "the", "he", "he", "she", "th"]);
    /// let words = ["th", "he"];
    /// let got = sc.par_corrections_batch(&words, 1);
    /// assert_eq!(got, [sc.corrections("th", 1), sc.corrections("he", 1)]);
    /// ```
    #[cfg(feature = "parallel")]
    #[must_use]
    pub fn par_corrections_batch(
        &self,
        words: &[&str],
        max_distance: u32,
    ) -> Vec<Vec<Correction<'_>>> {
        use rayon::prelude::*;
        words
            .par_iter()
            .map(|word| self.corrections(word, max_distance))
            .collect()
    }

    // -----------------------------------------------------------------------
    // Internals
    // -----------------------------------------------------------------------

    /// Calls `f` with every corpus word within `max_distance` of `query`, in no
    /// particular order — the shared retrieval both public shapes build on.
    fn for_each_match<'s, F: FnMut(Correction<'s>)>(
        &'s self,
        query: &str,
        max_distance: u32,
        mut f: F,
    ) {
        let units = to_scalars(query);
        let emit = |i: u32, f: &mut F| {
            let word: &'s str = &self.words[i as usize];
            let distance = damerau_levenshtein(query, word) as u32;
            if distance <= max_distance {
                f(Correction {
                    word,
                    distance,
                    frequency: self.frequencies[i as usize],
                });
            }
        };

        if max_distance <= NEAR_DISTANCE {
            let near = self.near.get_or_init(|| self.build_near_index());
            let mut candidates: Vec<u32> = Vec::new();
            for variant in distinct_deletions(&units, max_distance) {
                if let Some(indices) = near.get(&hash_scalars(&variant)) {
                    candidates.extend_from_slice(indices);
                }
            }
            candidates.sort_unstable();
            candidates.dedup();
            for i in candidates {
                emit(i, &mut f);
            }
            return;
        }

        // A scan, with the one lower bound that costs less than the metric:
        // the distance is at least the difference in scalar length.
        let query_len = units.len();
        for (i, word) in self.words.iter().enumerate() {
            if word.chars().count().abs_diff(query_len) > max_distance as usize {
                continue;
            }
            // Lossless: `build` refuses to create a word past
            // `MAX_DISTINCT_WORDS`, so every index into `words` fits a `u32`.
            emit(i as u32, &mut f);
        }
    }

    /// Every entry's deletion sequences to depth [`NEAR_DISTANCE`], hashed.
    ///
    /// Keyed by a 64-bit hash rather than by the sequence itself: a collision
    /// can only *add* a candidate, never remove one, and every candidate has
    /// its exact distance computed before it is emitted — so collisions cannot
    /// change an answer, and the map holds no key bytes at all.
    fn build_near_index(&self) -> FxHashMap<u64, Vec<u32>> {
        let mut near: FxHashMap<u64, Vec<u32>> = FxHashMap::default();
        for (i, word) in self.words.iter().enumerate() {
            // Lossless for the same reason as the scan in `for_each_candidate`:
            // `build` caps `words.len()` at `MAX_DISTINCT_WORDS`.
            let i = i as u32;
            let units = to_scalars(word);
            for variant in distinct_deletions(&units, NEAR_DISTANCE) {
                let bucket = near.entry(hash_scalars(&variant)).or_default();
                // Distinct sequences of one word can collide on one hash; this
                // word is the bucket's most recent writer, so checking the tail
                // keeps the bucket free of repeats without a per-key set.
                if bucket.last() != Some(&i) {
                    bucket.push(i);
                }
            }
        }
        near
    }
}

/// Hashes a scalar sequence, seeded with its length so that a sequence and its
/// own prefixes do not collide systematically.
fn hash_scalars(units: &[char]) -> u64 {
    let mut h = FxHasher::default();
    h.write_usize(units.len());
    for &c in units {
        h.write_u32(c as u32);
    }
    h.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn words(sc: &Spellcheck, query: &str, k: u32) -> Vec<String> {
        sc.correction_words(query, k)
    }

    /// The address ceiling, exercised at the boundary.
    ///
    /// The real ceiling is [`MAX_DISTINCT_WORDS`] — 4.29 billion distinct
    /// words, about 620 GiB — so it cannot be reached by a test on any
    /// machine this suite runs on. That is exactly why [`Spellcheck::build`]
    /// takes the limit as a parameter: the *logic* at the boundary is the
    /// thing worth pinning, and it is identical at 2 and at `u32::MAX`.
    ///
    /// Three properties, each of which a rewrite could plausibly break:
    /// the last word that fits is accepted, the first that does not is an
    /// error rather than a panic or a silent omission, and a repeat never
    /// consumes an id (so a corpus of a billion occurrences of two words is
    /// still a two-word corpus).
    #[test]
    fn a_corpus_past_the_address_ceiling_is_an_error_not_a_panic() {
        let full = Spellcheck::build(["a", "b", "a"], 2).expect("two distinct words fit a limit 2");
        assert_eq!(full.len(), 2);
        assert_eq!(full.frequency("a"), Some(2));

        assert_eq!(
            Spellcheck::build(["a", "b", "c"], 2).unwrap_err(),
            CorpusTooLarge { limit: 2 }
        );
        // The word that overflows may arrive at any point, including last.
        assert_eq!(
            Spellcheck::build(["a", "a", "b", "b", "a", "c"], 2).unwrap_err(),
            CorpusTooLarge { limit: 2 }
        );

        // Repeats are free: 6 occurrences, 2 distinct words, still fits.
        let repeated = Spellcheck::build(["a", "a", "a", "b", "b", "b"], 2)
            .expect("repeats do not consume ids");
        assert_eq!(repeated.len(), 2);
        assert_eq!(repeated.frequency("b"), Some(3));

        // A limit of zero admits nothing but an empty corpus, which is the
        // degenerate end of the same rule.
        assert!(Spellcheck::build(std::iter::empty::<&str>(), 0).is_ok());
        assert_eq!(
            Spellcheck::build(["a"], 0).unwrap_err(),
            CorpusTooLarge { limit: 0 }
        );

        // And the ceiling the public constructors actually use.
        assert_eq!(MAX_DISTINCT_WORDS, u32::MAX);
        assert_eq!(
            CorpusTooLarge { limit: 4 }.to_string(),
            "corpus holds more than 4 distinct words, which is all a Spellcheck can address"
        );
    }

    /// `try_new` and `new` agree on every corpus a machine can hold.
    #[test]
    fn try_new_matches_new_below_the_ceiling() {
        let corpus = ["the", "the", "cat", "sat", "cat"];
        let a = Spellcheck::new(corpus);
        let b = Spellcheck::try_new(corpus).expect("a five-word corpus fits");
        assert_eq!(a.len(), b.len());
        for (w, f) in a.frequencies() {
            assert_eq!(b.frequency(w), Some(f));
        }
    }

    /// The definition: every corpus word within `k`, and nothing else.
    fn assert_is_the_metric_ball(sc: &Spellcheck, corpus: &[&str], query: &str, k: u32) {
        let mut want: Vec<&str> = corpus
            .iter()
            .copied()
            .filter(|w| damerau_levenshtein(query, w) <= k as usize)
            .collect();
        want.sort_unstable();
        want.dedup();
        let mut got: Vec<String> = words(sc, query, k);
        got.sort_unstable();
        got.dedup();
        assert_eq!(got, want, "query {query:?} at k={k}");
    }

    #[test]
    fn empty_word_list() {
        let sc = Spellcheck::new(Vec::<String>::new());
        assert!(sc.is_empty());
        assert!(!sc.is_correct(""));
        assert!(!sc.is_correct("cat"));
        assert!(sc.corrections("cat", 1).is_empty());
        assert_eq!(sc.frequencies().count(), 0);
    }

    #[test]
    fn empty_string_in_the_list_is_a_word() {
        let sc = Spellcheck::new(["", "cat"]);
        assert!(sc.is_correct(""));
        assert_eq!(sc.len(), 2);
        // "a" -> "" is one deletion.
        assert_eq!(words(&sc, "a", 1), [""]);
        assert_eq!(words(&sc, "", 0), [""]);
    }

    #[test]
    fn zero_means_zero_edits() {
        let sc = Spellcheck::new(["cat"]);
        assert!(sc.corrections("ct", 0).is_empty());
        assert_eq!(words(&sc, "cat", 0), ["cat"]);
    }

    #[test]
    fn a_correct_word_is_its_own_zero_distance_correction() {
        let sc = Spellcheck::new(["cat", "cot"]);
        let got = sc.corrections("cat", 1);
        assert_eq!(got[0].word, "cat");
        assert_eq!(got[0].distance, 0);
    }

    #[test]
    fn frequencies_are_counts_never_nan() {
        let sc = Spellcheck::new([
            "constructor",
            "toString",
            "toString",
            "valueOf",
            "valueOf",
            "valueOf",
            "__proto__",
            "cat",
        ]);
        assert_eq!(sc.frequency("constructor"), Some(1));
        assert_eq!(sc.frequency("toString"), Some(2));
        assert_eq!(sc.frequency("valueOf"), Some(3));
        assert_eq!(sc.frequency("__proto__"), Some(1));
        assert_eq!(sc.frequency("cat"), Some(1));
        assert_eq!(sc.frequency("dog"), None);
        // Every one of them is an ordinary word.
        for w in ["constructor", "toString", "valueOf", "__proto__", "cat"] {
            assert!(sc.is_correct(w), "{w}");
        }
        let keys: Vec<&str> = sc.frequencies().map(|(w, _)| w).collect();
        assert_eq!(
            keys,
            ["constructor", "toString", "valueOf", "__proto__", "cat"],
            "first-occurrence order"
        );
    }

    #[test]
    fn a_transposition_costs_one_edit() {
        assert_eq!(damerau_levenshtein("hte", "the"), 1);
        let sc = Spellcheck::new(["the"]);
        assert_eq!(words(&sc, "hte", 1), ["the"]);
    }

    #[test]
    fn corrections_are_not_restricted_to_any_alphabet() {
        let sc = Spellcheck::new(["café", "Москва", "日本語", "😀ab"]);
        assert_eq!(words(&sc, "cafe", 1), ["café"]);
        assert_eq!(words(&sc, "Мсква", 1), ["Москва"]);
        assert_eq!(words(&sc, "日本", 1), ["日本語"]);
        assert_eq!(words(&sc, "ab", 1), ["😀ab"], "one astral scalar, one edit");
    }

    #[test]
    fn case_differences_are_ordinary_substitutions() {
        let sc = Spellcheck::new(["Cat"]);
        assert!(!sc.is_correct("cat"));
        assert_eq!(words(&sc, "cat", 1), ["Cat"]);
        assert_eq!(words(&sc, "Cta", 1), ["Cat"]);
    }

    #[test]
    fn ordering_is_distance_then_frequency_then_word() {
        // "ab" and "ba" are both one edit from "aa"; "ab" is more frequent, so
        // it comes first. "ac" ties "ba" on distance and frequency, so the
        // word itself breaks it.
        let sc = Spellcheck::new(["ab", "ab", "ba", "ac", "aaaa"]);
        let got = sc.corrections("aa", 1);
        assert_eq!(
            got.iter()
                .map(|c| (c.word, c.distance, c.frequency))
                .collect::<Vec<_>>(),
            [("ab", 1, 2), ("ac", 1, 1), ("ba", 1, 1)]
        );
    }

    #[test]
    fn distance_dominates_frequency_absolutely() {
        let sc = Spellcheck::new(["zz", "zz", "zz", "zz", "az", "a", "a"]);
        assert_eq!(words(&sc, "aa", 2), ["a", "az", "zz"]);
    }

    #[test]
    fn a_word_appears_at_most_once() {
        let sc = Spellcheck::new(["cat", "cat", "at"]);
        let got = words(&sc, "cat", 2);
        let mut unique = got.clone();
        unique.sort();
        unique.dedup();
        assert_eq!(got.len(), unique.len(), "{got:?}");
    }

    #[test]
    fn best_correction_agrees_with_the_head_of_corrections() {
        let corpus = [
            "cat", "cats", "car", "cart", "cot", "dog", "café", "😀ab", "ab", "",
        ];
        let sc = Spellcheck::new(corpus);
        for query in ["cat", "ca", "caft", "cafe", "😀ab", "ab", "xyz", ""] {
            for k in 0u32..=3 {
                assert_eq!(
                    sc.best_correction(query, k),
                    sc.corrections(query, k).first().copied(),
                    "best_correction({query:?}, {k})"
                );
            }
        }
    }

    #[test]
    fn the_answer_is_the_metric_ball_across_the_dispatch_boundary() {
        let corpus = [
            "cat", "cats", "car", "cart", "cot", "dog", "café", "😀ab", "ab", "", "aaaa", "hte",
            "the",
        ];
        let sc = Spellcheck::new(corpus);
        for query in [
            "cat", "ca", "caft", "cafe", "😀ab", "ab", "xyz", "", "hte", "aaaaaa",
        ] {
            // 0..=2 is the indexed path, 3..=4 the scan: both must produce the
            // same definition of the answer.
            for k in 0u32..=4 {
                assert_is_the_metric_ball(&sc, &corpus, query, k);
            }
        }
    }

    #[test]
    fn very_long_input() {
        let long = "a".repeat(500);
        let sc = Spellcheck::new([long.clone()]);
        assert!(sc.is_correct(&long));
        assert_eq!(words(&sc, &"a".repeat(499), 1), [long]);
    }

    #[test]
    fn punctuation_and_digits_are_ordinary_scalars() {
        let sc = Spellcheck::new(["e.g.", "123", "1,000", "don't"]);
        assert!(sc.is_correct("e.g."));
        assert_eq!(words(&sc, "123", 0), ["123"]);
        // The metric has no alphabet, so a missing apostrophe is one edit.
        assert_eq!(words(&sc, "dont", 1), ["don't"]);
        assert_eq!(words(&sc, "e.g", 1), ["e.g."]);
    }

    /// `Spellcheck` must be usable from several threads against one shared
    /// instance. `Arc`-spawning a closure that captures it only compiles if
    /// `Spellcheck: Send + Sync`, so a successful compile *is* the check — and
    /// it also exercises the `OnceLock` being initialised under contention.
    #[test]
    fn shared_spellcheck_is_usable_concurrently() {
        let sc = std::sync::Arc::new(Spellcheck::new(["the", "the", "the", "he", "he", "she"]));
        let expected = sc.correction_words("th", 1);
        let handles: Vec<_> = (0..8)
            .map(|_| {
                let sc = std::sync::Arc::clone(&sc);
                let expected = expected.clone();
                std::thread::spawn(move || {
                    for _ in 0..50 {
                        assert_eq!(sc.correction_words("th", 1), expected);
                    }
                })
            })
            .collect();
        for h in handles {
            h.join().unwrap();
        }
    }

    /// Sequential-vs-parallel equivalence over every input class the
    /// sequential suite above exercises.
    #[cfg(feature = "parallel")]
    mod parallel_equivalence {
        use super::*;

        fn assert_equivalent(sc: &Spellcheck, words: &[&str], max_distance: u32) {
            let sequential: Vec<Vec<Correction<'_>>> = words
                .iter()
                .map(|w| sc.corrections(w, max_distance))
                .collect();
            let parallel = sc.par_corrections_batch(words, max_distance);
            assert_eq!(
                parallel, sequential,
                "batch {words:?} at distance {max_distance} diverged"
            );
        }

        #[test]
        fn empty_input() {
            let sc = Spellcheck::new(["the", "he", "she"]);
            assert_equivalent(&sc, &[], 1);
            assert_equivalent(&sc, &[], 2);
        }

        #[test]
        fn one_word() {
            let sc = Spellcheck::new(["the", "the", "the", "he", "he", "she", "th"]);
            assert_equivalent(&sc, &["th"], 1);
        }

        #[test]
        fn many_words() {
            let sc = Spellcheck::new([
                "zz",
                "zz",
                "zz",
                "zz",
                "az",
                "a",
                "a",
                "something",
                "cat",
                "cart",
                "car",
                "dog",
            ]);
            let words = [
                "aa",
                "smething",
                "somethhing",
                "somehting",
                "sometzing",
                "smtehing",
                "ca",
                "",
                "xyz123",
                "cat",
                "zz",
                "aa",
                "smething",
                "ca",
                "cat",
                "dog",
                "car",
            ];
            assert_equivalent(&sc, &words, 1);
            assert_equivalent(&sc, &words, 2);
            assert_equivalent(&sc, &words, 3);
        }

        #[test]
        fn unicode() {
            let sc = Spellcheck::new([
                "café",
                "Москва",
                "Ελλάδα",
                "日本語",
                "中文测试",
                "a😀b",
                "ab",
            ]);
            let words = [
                "café",
                "Москва",
                "Ελλάδα",
                "日本語",
                "中文测试",
                "a😀b",
                "ab",
            ];
            assert_equivalent(&sc, &words, 1);
            assert_equivalent(&sc, &words, 2);
        }

        #[test]
        fn empty_string_entries_and_probes() {
            let sc = Spellcheck::new(["", "a", "b", "cat"]);
            assert_equivalent(&sc, &["", "a", "cat", "zzz"], 1);
            assert_equivalent(&sc, &["", "a", "cat", "zzz"], 2);
        }

        #[test]
        fn pathologically_long_word_mixed_with_short_ones() {
            let long = "a".repeat(500);
            let sc = Spellcheck::new([long.clone(), "cat".to_string()]);
            let almost_long = "a".repeat(499);
            let words = [almost_long.as_str(), "cat", "dog", long.as_str()];
            assert_equivalent(&sc, &words, 1);
        }

        #[test]
        fn zero_distance() {
            let sc = Spellcheck::new(["cat"]);
            assert_equivalent(&sc, &["ct", "cat", "xyz"], 0);
        }
    }
}
