//! The spellchecker itself: The reference `spellcheck`.

use rustc_hash::{FxHashMap, FxHashSet};
use verbora_trie::Trie;

use crate::comparator_sort;
use crate::edits::Edits;
use crate::units::{EditUnit, to_utf16};

/// The one `Object.prototype` key whose assignment is silently dropped.
const PROTO_KEY: &str = "__proto__";

/// Every other own property of `Object.prototype`.
///
/// Reading any of these off a `{}` finds an inherited **function**, which is
/// truthy — so the reference's `if (!this.word2frequency[w]) … = 0` guard does
/// not fire, and the following `++` stores `Number(function)`, i.e. `NaN`.
/// Sorted, so the lookup can be a binary search over a static table.
const PROTO_METHODS: [&str; 11] = [
    "__defineGetter__",
    "__defineSetter__",
    "__lookupGetter__",
    "__lookupSetter__",
    "constructor",
    "hasOwnProperty",
    "isPrototypeOf",
    "propertyIsEnumerable",
    "toLocaleString",
    "toString",
    "valueOf",
];

/// One distinct word from the list.
#[derive(Debug)]
struct Entry {
    word: Box<str>,
    /// The value the reference's `word2frequency` ends up holding. `f64`, not an
    /// integer, because prototype-shadowed keys really do end up as `NaN`.
    frequency: f64,
    /// `false` only for `__proto__`, which never becomes an own property.
    own: bool,
}

/// A probabilistic spellchecker over a word list, after
/// [Norvig](https://norvig.com/spell-correct.html).
///
/// The word list is a *corpus*, not a dictionary: a word's number of
/// occurrences is its probability, and corrections come back most-probable
/// first. Feeding it real text works far better than feeding it
/// `/usr/share/dict/words`, where every word has frequency 1 and the ordering
/// degenerates to the edit generator's own.
///
/// ```
/// use verbora_spellcheck::Spellcheck;
///
/// let sc = Spellcheck::new(["something"]);
/// assert!(sc.is_correct("something"));
/// assert!(!sc.is_correct("smething"));
/// assert!(sc.get_corrections("smething", 1).contains(&"something".to_string()));
/// ```
///
/// # Case sensitivity
///
/// Matching is case-**sensitive**, and the edit alphabet is lowercase ASCII
/// only. So a list of `["Cat"]` cannot correct `"cat"` (no edit can produce a
/// capital `C`) but does correct `"Cta"`, because transposition rearranges what
/// is already there.
///
/// ```
/// use verbora_spellcheck::Spellcheck;
///
/// let sc = Spellcheck::new(["Cat"]);
/// assert!(sc.get_corrections("cat", 1).is_empty());
/// assert_eq!(sc.get_corrections("Cta", 1), ["Cat"]);
/// ```
///
/// # The frequency table inherits from `Object.prototype`
///
/// The reference builds its counts in a plain `{}`, so twelve word values
/// collide with inherited properties and count wrongly. This is reproduced,
/// because it changes the order corrections come back in:
///
/// * `'constructor'`, `'toString'`, `'valueOf'`, `'hasOwnProperty'`,
///   `'isPrototypeOf'`, `'propertyIsEnumerable'`, `'toLocaleString'`,
///   `'__defineGetter__'`, `'__defineSetter__'`, `'__lookupGetter__'`,
///   `'__lookupSetter__'` — the first occurrence increments an inherited
///   *function*, storing `NaN`; later occurrences count normally from zero. A
///   word seen `n >= 2` times therefore has frequency `n - 1`, and one seen
///   exactly once has `NaN`.
/// * `'__proto__'` — the assignment is dropped by the setter, so no own
///   property is ever created and every read yields `Object.prototype`, which
///   coerces to `NaN` in the comparator. Its frequency is `NaN` at any count.
///
/// ```
/// use verbora_spellcheck::Spellcheck;
///
/// let sc = Spellcheck::new(["constructor", "toString", "toString", "cat"]);
/// assert!(sc.frequency("constructor").unwrap().is_nan()); // seen once
/// assert_eq!(sc.frequency("toString"), Some(1.0));        // seen twice
/// assert_eq!(sc.frequency("cat"), Some(1.0));
/// ```
#[derive(Debug)]
pub struct Spellcheck {
    trie: Trie,
    entries: Vec<Entry>,
    index: FxHashMap<Box<str>, u32>,
    /// Entry indices in the reference's `Object.keys` order; see
    /// [`Spellcheck::frequencies`].
    key_order: Vec<u32>,
}

impl Spellcheck {
    /// Builds a spellchecker from a corpus.
    ///
    /// Repeats are meaningful: they are the frequencies. The list is walked
    /// once, in order, so the first occurrence of each word fixes its position
    /// in [`Spellcheck::frequencies`].
    ///
    /// # Examples
    ///
    /// ```
    /// use verbora_spellcheck::Spellcheck;
    ///
    /// let sc = Spellcheck::new(["the", "cat", "the"]);
    /// assert_eq!(sc.frequency("the"), Some(2.0));
    /// assert_eq!(sc.frequency("dog"), None);
    /// ```
    pub fn new<I, S>(wordlist: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let mut trie = Trie::new();
        let mut entries: Vec<Entry> = Vec::new();
        let mut index: FxHashMap<Box<str>, u32> = FxHashMap::default();
        // Raw occurrence counts; the prototype adjustment is applied afterwards
        // so the walk itself stays a plain increment.
        let mut counts: Vec<u64> = Vec::new();

        for word in wordlist {
            let word = word.as_ref();
            trie.add_string(word);
            if let Some(&i) = index.get(word) {
                counts[i as usize] += 1;
            } else {
                let i = u32::try_from(entries.len()).expect("word list exceeds u32 entries");
                index.insert(word.into(), i);
                entries.push(Entry {
                    word: word.into(),
                    frequency: 0.0,
                    own: word != PROTO_KEY,
                });
                counts.push(1);
            }
        }

        for (entry, &count) in entries.iter_mut().zip(&counts) {
            entry.frequency = shadowed_frequency(&entry.word, count);
        }

        let key_order = key_order(&entries);
        Self {
            trie,
            entries,
            index,
            key_order,
        }
    }

    /// The prefix tree the membership test runs over.
    ///
    /// Exposed because the reference exposes `spellcheck.trie`, and because the
    /// trie answers prefix questions this API does not.
    pub fn trie(&self) -> &Trie {
        &self.trie
    }

    /// Whether `word` is in the list, exactly and case-sensitively.
    ///
    /// # Examples
    ///
    /// ```
    /// use verbora_spellcheck::Spellcheck;
    ///
    /// let sc = Spellcheck::new(["", "cat"]);
    /// assert!(sc.is_correct("cat"));
    /// assert!(!sc.is_correct("Cat"));
    /// // An empty string in the list marks the trie's root as a word.
    /// assert!(sc.is_correct(""));
    /// ```
    pub fn is_correct(&self, word: &str) -> bool {
        self.trie.contains(word)
    }

    /// The frequency the reference would use when ordering `word`.
    ///
    /// `None` means the word is not in the list. `Some(NaN)` means the count was
    /// destroyed by `Object.prototype` (see the type-level docs); `NaN` is
    /// exactly what the reference's comparator sees, so it is returned rather
    /// than hidden.
    ///
    /// # Divergence
    ///
    /// For a prototype method name that is *not* in the word list, the reference
    /// yields the inherited function rather than `undefined`. This returns
    /// `None` for both. The distinction is unobservable through
    /// [`Spellcheck::get_corrections`], which only ever looks up words the trie
    /// already accepted.
    pub fn frequency(&self, word: &str) -> Option<f64> {
        if word == PROTO_KEY {
            // Reading `__proto__` off any object literal yields
            // `Object.prototype`, in the word list or not.
            return Some(f64::NAN);
        }
        self.index
            .get(word)
            .map(|&i| self.entries[i as usize].frequency)
    }

    /// Every word and its frequency, in the reference's `Object.keys` order.
    ///
    /// That order is **not** insertion order: keys that look like array indices
    /// come first, ascending numerically, and only then the rest in insertion
    /// order. `__proto__` never appears, because it never becomes an own
    /// property.
    ///
    /// # Examples
    ///
    /// ```
    /// use verbora_spellcheck::Spellcheck;
    ///
    /// let sc = Spellcheck::new(["b", "10", "2", "a", "__proto__"]);
    /// let keys: Vec<&str> = sc.frequencies().map(|(w, _)| w).collect();
    /// assert_eq!(keys, ["2", "10", "b", "a"]);
    /// ```
    pub fn frequencies(&self) -> impl Iterator<Item = (&str, f64)> {
        self.key_order.iter().map(|&i| {
            let entry = &self.entries[i as usize];
            (&*entry.word, entry.frequency)
        })
    }

    /// Suggested corrections for `word`, most probable first.
    ///
    /// `max_distance` is the maximum number of edits. **`0` means `1`**,
    /// reproducing the reference's `if (!maxDistance) maxDistance = 1` — which
    /// tests falsiness, not absence, so `0`, `null`, `false`, `NaN` and `''` all
    /// arrive as `1` there too.
    ///
    /// # Ordering
    ///
    /// Words reachable in `k` edits are treated as infinitely more probable than
    /// words needing `k + 1`, so the result is grouped by distance first. Within
    /// a distance group the order is by descending corpus frequency, ties broken
    /// by the edit generator's own order — a *stable* sort, which is observable
    /// and reproduced exactly (see [`crate::Spellcheck`] and the crate docs).
    /// A word found at one distance never reappears at a greater one.
    ///
    /// Note that `word` itself is among its own edits, so a correctly spelled
    /// word is normally returned as its own first correction.
    ///
    /// # Cost
    ///
    /// There are roughly `54n + 25` strings one edit from a word of length `n`,
    /// and each level multiplies that again: `max_distance = 2` examines
    /// ~10<sup>4</sup>–10<sup>5</sup> candidates and `3` reaches 10<sup>6</sup>.
    /// Norvig's figure is that 80–95% of real misspellings are one edit out.
    ///
    /// # Examples
    ///
    /// ```
    /// use verbora_spellcheck::Spellcheck;
    ///
    /// // Frequencies decide the order: "the" (3) beats "he" (2) beats "she" (1),
    /// // regardless of the order the edit generator found them in.
    /// let sc = Spellcheck::new(["the", "the", "the", "he", "he", "she", "th"]);
    /// assert_eq!(sc.get_corrections("th", 1), ["the", "th"]);
    /// assert_eq!(sc.get_corrections("he", 1), ["the", "he", "she"]);
    /// ```
    pub fn get_corrections(&self, word: &str, max_distance: u32) -> Vec<String> {
        // `if (!maxDistance) maxDistance = 1`.
        let distance = if max_distance == 0 { 1 } else { max_distance };
        if word.is_ascii() {
            // ASCII is closed under the edit operation — the alphabet is
            // `a`–`z` — so this choice holds for every level.
            self.corrections_over(word.as_bytes(), distance)
        } else {
            self.corrections_over(&to_utf16(word), distance)
        }
    }

    /// [`Spellcheck::get_corrections`], fanned out across a `rayon` thread
    /// pool. Requires the `parallel` feature.
    ///
    /// # Why this exists
    ///
    /// [`Spellcheck`] is immutable after construction and holds nothing but
    /// owned data — a [`Trie`](verbora_trie::Trie), a `Vec<Entry>`, an
    /// `FxHashMap` and a `Vec<u32>`, no `Rc`, `RefCell`, `Cell` or other
    /// interior mutability anywhere in the type — so it is `Send + Sync`
    /// automatically, and looking up many words against it has zero
    /// coordination cost between lookups. `get_corrections` alone is
    /// expensive: this crate's own `spellcheck_get_corrections_d2` benchmark
    /// measures 3.9–5.5 ms for a single nine-letter word at distance 2, so a
    /// caller correcting more than a handful of words sequentially is easily
    /// looking at tens of milliseconds to seconds of wall time on one core.
    /// This function is exactly
    /// `words.par_iter().map(|w| self.get_corrections(w, max_distance)).collect()`
    /// — a thin fan-out over the existing sequential primitive, not a second
    /// implementation of it. `corrections_over`'s streaming of the deepest
    /// edit level, `comparator_sort`'s comparator port and every other detail of
    /// `get_corrections` are untouched; if you need a different shape in
    /// parallel (per-word `max_distance`, for instance), apply the same
    /// `par_iter().map(...)` pattern at your own call site (see
    /// `site/performance/parallelism.md`).
    ///
    /// # When to reach for it vs. the sequential loop
    ///
    /// Reach for this when correcting a *batch* of words against one corpus is
    /// itself the unit of work — spell-checking a whole document or a column
    /// of free-text input, for example — and `max_distance` is at least `2`
    /// or the batch runs to dozens of words or more. A `rayon` task costs on
    /// the order of a microsecond to schedule; at `max_distance = 1`, where a
    /// single call can run in well under a millisecond on a small corpus, a
    /// short batch may not clear that overhead. Measure before reaching for
    /// this on small batches at distance 1, and prefer a plain
    /// `words.iter().map(|w| self.get_corrections(w, max_distance))` loop
    /// there.
    ///
    /// Also do not reach for this if the caller already parallelises at a
    /// coarser grain — for example a server handling one request per thread —
    /// where nesting a second layer of parallelism inside each request
    /// typically *reduces* throughput by oversubscribing the CPU (see
    /// `site/performance/parallelism.md`).
    ///
    /// # Allocation behaviour
    ///
    /// One `Vec<Vec<String>>` sized to `words.len()` for the output, plus
    /// whatever [`Spellcheck::get_corrections`] itself allocates per word (one
    /// `Vec<String>` of corrections, and internally one dedup hash set and one
    /// edit-generation buffer, all freed at the end of that word's call). No
    /// additional buffering, no locking, no per-call thread-pool construction
    /// — this uses whichever global `rayon` pool is already installed (or
    /// `rayon`'s default one), so pool configuration remains the caller's
    /// responsibility, not this crate's.
    ///
    /// # Order and errors
    ///
    /// Output order matches input order — `results[i]` is
    /// `self.get_corrections(words[i], max_distance)` — via `rayon`'s
    /// order-preserving `map` + `collect`. There is no error case to
    /// preserve: [`Spellcheck::get_corrections`] already returns an empty
    /// `Vec` rather than a `Result` for a word with no corrections, and that
    /// per-word shape is unchanged here.
    ///
    /// # Examples
    ///
    /// ```
    /// use verbora_spellcheck::Spellcheck;
    ///
    /// let sc = Spellcheck::new(["the", "the", "the", "he", "he", "she", "th"]);
    /// let words = ["th", "he"];
    /// let got = sc.par_get_corrections_batch(&words, 1);
    /// assert_eq!(got, [sc.get_corrections("th", 1), sc.get_corrections("he", 1)]);
    /// ```
    #[cfg(feature = "parallel")]
    pub fn par_get_corrections_batch(&self, words: &[&str], max_distance: u32) -> Vec<Vec<String>> {
        use rayon::prelude::*;
        words
            .par_iter()
            .map(|word| self.get_corrections(word, max_distance))
            .collect()
    }

    /// All strings one edit from `word`, in the reference's order.
    ///
    /// Free-standing [`crate::edits`] with a receiver, mirroring the reference's
    /// `Spellcheck.prototype.edits`; it uses no instance state.
    pub fn edits(&self, word: &str) -> Vec<String> {
        crate::edits(word)
    }

    /// All strings one edit from `word`, as raw UTF-16 code units.
    ///
    /// See [`crate::edits_utf16`].
    pub fn edits_utf16(&self, word: &str) -> Vec<Vec<u16>> {
        crate::edits_utf16(word)
    }

    /// Every edit level from 0 up to `distance`.
    ///
    /// See [`crate::edits_with_max_distance`] — including the warning about how
    /// fast the levels grow.
    pub fn edits_with_max_distance(&self, word: &str, distance: u32) -> Vec<Vec<String>> {
        crate::edits_with_max_distance(word, distance)
    }

    /// Every edit level from 0 up to `distance`, as raw UTF-16 code units.
    ///
    /// See [`crate::edits_with_max_distance_utf16`].
    pub fn edits_with_max_distance_utf16(&self, word: &str, distance: u32) -> Vec<Vec<Vec<u16>>> {
        crate::edits_with_max_distance_utf16(word, distance)
    }

    /// Appends `distance_counter` further edit levels to `levels`, in place.
    ///
    /// The reference keeps this on the prototype, so it is mirrored here; like
    /// the other edit APIs it uses no instance state. See
    /// [`crate::edits_with_max_distance_helper`] for the two behaviours that
    /// make it *not* interchangeable with
    /// [`Spellcheck::edits_with_max_distance_utf16`].
    pub fn edits_with_max_distance_helper(
        &self,
        distance_counter: u32,
        levels: &mut Vec<Vec<Vec<u16>>>,
    ) {
        crate::edits_with_max_distance_helper(distance_counter, levels);
    }

    /// The correction search, in whichever unit representation was chosen.
    ///
    /// Structurally this is the reference's `getCorrections`, with one
    /// difference that is invisible in the output: the deepest level is
    /// *streamed* rather than built. The reference materialises a reference
    /// array of every distance-`d` candidate before filtering it — 270,000
    /// strings for a nine-letter word at `d = 2` — whereas only the previous
    /// level is ever needed in full, to seed the next one.
    fn corrections_over<U: EditUnit>(&self, word: &[U], distance: u32) -> Vec<String> {
        let mut level: Vec<Vec<U>> = vec![word.to_vec()];
        let mut out: Vec<String> = Vec::new();
        let mut emitted: FxHashSet<u32> = FxHashSet::default();
        let mut buf = String::new();
        let mut hits: Vec<(u32, f64)> = Vec::new();
        // Parked between sources so the dedup table is allocated once for the
        // whole search rather than once per source word. Parking it on an empty
        // `'static` slice is also what lets it outlive the borrow of `level`.
        let mut spare: Option<Edits<'static, U>> = None;

        for depth in 1..=distance {
            let is_last = depth == distance;
            hits.clear();
            let mut next: Vec<Vec<U>> = Vec::new();

            for source in &level {
                let mut generator = match spare.take() {
                    Some(parked) => parked.restart(source),
                    None => Edits::new(source),
                };
                while let Some(candidate) = generator.next_edit() {
                    if let Some(text) = U::as_str(candidate, &mut buf) {
                        // The trie is the reference's own membership test; the
                        // map only supplies the frequency, and the two are built
                        // from the same list, so this lookup never misses.
                        if self.trie.contains(text) {
                            if let Some(&i) = self.index.get(text) {
                                hits.push((i, self.entries[i as usize].frequency));
                            }
                        }
                    }
                    if !is_last {
                        next.push(candidate.to_vec());
                    }
                }
                spare = Some(generator.restart(&[]));
            }

            // The reference's comparator, verbatim. It never returns 0, which
            // matters only when a frequency is NaN — and then the exact
            // permutation is the reference engine's TimSort, not "a stable sort".
            comparator_sort::sort_by(&mut hits, |a, b| if a.1 > b.1 { -1 } else { 1 });

            for &(i, _) in &hits {
                if emitted.insert(i) {
                    out.push(self.entries[i as usize].word.to_string());
                }
            }
            level = next;
        }
        out
    }
}

/// The frequency the reference's `{}`-backed counter ends up holding.
fn shadowed_frequency(word: &str, count: u64) -> f64 {
    if word == PROTO_KEY {
        // Every `word2frequency['__proto__'] = …` is dropped by the setter, so
        // the read always returns Object.prototype — NaN under `>`.
        return f64::NAN;
    }
    if PROTO_METHODS.binary_search(&word).is_ok() {
        // The first `++` incremented an inherited function: `Number(fn)` is NaN,
        // and the *second* occurrence sees that falsy NaN and restarts from 0.
        return if count == 1 {
            f64::NAN
        } else {
            (count - 1) as f64
        };
    }
    count as f64
}

/// Orders entries the way `Object.keys` would.
///
/// The reference enumerates array-index-like keys first, ascending numerically,
/// then everything else in insertion order. A key is array-index-like when it is
/// the canonical decimal form of a `u32` below `2^32 - 1` — so `"10"` qualifies
/// and `"01"`, `"1.5"`, `"-1"` and `"4294967295"` do not.
fn key_order(entries: &[Entry]) -> Vec<u32> {
    let mut indexed: Vec<(u32, u32)> = Vec::new();
    let mut rest: Vec<u32> = Vec::new();
    for (i, entry) in entries.iter().enumerate() {
        if !entry.own {
            continue;
        }
        let i = u32::try_from(i).expect("entry count fits in u32");
        match array_index(&entry.word) {
            Some(n) => indexed.push((n, i)),
            None => rest.push(i),
        }
    }
    indexed.sort_unstable();
    let mut order: Vec<u32> = indexed.into_iter().map(|(_, i)| i).collect();
    order.extend(rest);
    order
}

/// The array index a key denotes, if it denotes one.
fn array_index(key: &str) -> Option<u32> {
    let bytes = key.as_bytes();
    if bytes.is_empty() || !bytes.iter().all(u8::is_ascii_digit) {
        return None;
    }
    // "01" is not the canonical form of 1, so it is an ordinary string key.
    if bytes.len() > 1 && bytes[0] == b'0' {
        return None;
    }
    let n: u32 = key.parse().ok()?;
    // 2^32 - 1 is the length sentinel, never an index.
    (n != u32::MAX).then_some(n)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_word_list() {
        let sc = Spellcheck::new(Vec::<String>::new());
        assert!(!sc.is_correct(""));
        assert!(!sc.is_correct("cat"));
        assert!(sc.get_corrections("cat", 1).is_empty());
        assert_eq!(sc.frequencies().count(), 0);
    }

    #[test]
    fn empty_string_in_the_list_is_a_word() {
        let sc = Spellcheck::new(["", "cat"]);
        assert!(sc.is_correct(""));
        // 'cat' -> '' is two edits, so distance 1 cannot reach it from 'cat'.
        assert_eq!(sc.get_corrections("a", 1), [""]);
    }

    #[test]
    fn single_character_and_uppercase() {
        let sc = Spellcheck::new(["a", "A"]);
        assert!(sc.is_correct("a"));
        assert!(sc.is_correct("A"));
        assert_eq!(sc.get_corrections("A", 1), ["A", "a"]);
    }

    #[test]
    fn accented_latin_cyrillic_greek_and_cjk_round_trip() {
        let sc = Spellcheck::new(["café", "Москва", "Ελλάδα", "日本語", "中文测试"]);
        for w in ["café", "Москва", "Ελλάδα", "日本語", "中文测试"] {
            assert!(sc.is_correct(w), "{w}");
            // Every word is its own degenerate transposition.
            assert!(sc.get_corrections(w, 1).contains(&w.to_string()), "{w}");
        }
    }

    #[test]
    fn astral_and_emoji() {
        let sc = Spellcheck::new(["a😀b", "ab"]);
        assert!(sc.is_correct("a😀b"));
        assert!(sc.get_corrections("a😀b", 1).contains(&"a😀b".to_string()));
        // 'ab' is two code-unit deletions away, so not reachable at distance 1.
        assert!(!sc.get_corrections("a😀b", 1).contains(&"ab".to_string()));
        assert!(sc.get_corrections("a😀b", 2).contains(&"ab".to_string()));
    }

    #[test]
    fn punctuation_and_digits() {
        let sc = Spellcheck::new(["e.g.", "123", "1,000", "don't"]);
        assert!(sc.is_correct("e.g."));
        // Only the degenerate transposition reaches it: the edit alphabet has
        // no digits, so no insertion or replacement can produce one.
        assert_eq!(sc.get_corrections("123", 1), ["123"]);
        assert!(
            sc.get_corrections("dont", 1).is_empty(),
            "'\\'' is not in a-z"
        );
        assert!(sc.get_corrections("e.g", 1).is_empty(), "'.' is not in a-z");
    }

    #[test]
    fn very_long_input() {
        let long = "a".repeat(500);
        let sc = Spellcheck::new([long.clone()]);
        assert!(sc.is_correct(&long));
        assert_eq!(sc.get_corrections(&"a".repeat(499), 1), [long]);
    }

    #[test]
    fn max_distance_zero_means_one() {
        let sc = Spellcheck::new(["cat"]);
        assert_eq!(sc.get_corrections("ct", 0), sc.get_corrections("ct", 1));
        assert_eq!(sc.get_corrections("ct", 0), ["cat"]);
    }

    #[test]
    fn corrections_are_grouped_by_distance_then_frequency() {
        // 'zz' is the most frequent word by far, but it is two edits from 'aa'
        // while 'a' and 'az' are one, and distance dominates frequency
        // absolutely: a nearer word is treated as infinitely more probable.
        let sc = Spellcheck::new(["zz", "zz", "zz", "zz", "az", "a", "a"]);
        assert_eq!(sc.get_corrections("aa", 2), ["a", "az", "zz"]);
    }

    #[test]
    fn a_word_is_never_repeated_across_distances() {
        let sc = Spellcheck::new(["cat", "cat", "at"]);
        let got = sc.get_corrections("cat", 2);
        let mut unique = got.clone();
        unique.sort();
        unique.dedup();
        assert_eq!(got.len(), unique.len(), "{got:?}");
    }

    #[test]
    fn prototype_shadowed_frequencies() {
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
        assert!(sc.frequency("constructor").unwrap().is_nan());
        assert_eq!(sc.frequency("toString"), Some(1.0));
        assert_eq!(sc.frequency("valueOf"), Some(2.0));
        assert!(sc.frequency("__proto__").unwrap().is_nan());
        assert_eq!(sc.frequency("cat"), Some(1.0));
        // All of them are still words, `__proto__` included.
        assert!(sc.is_correct("__proto__"));
        // …but `__proto__` never becomes an own key of the frequency table.
        let keys: Vec<&str> = sc.frequencies().map(|(w, _)| w).collect();
        assert!(!keys.contains(&"__proto__"), "{keys:?}");
    }

    #[test]
    fn proto_key_reads_as_nan_even_when_absent() {
        let sc = Spellcheck::new(["cat"]);
        assert!(sc.frequency("__proto__").unwrap().is_nan());
        assert!(!sc.is_correct("__proto__"));
    }

    #[test]
    fn array_index_keys_are_hoisted() {
        let sc = Spellcheck::new(["b", "10", "2", "a", "0", "01", "4294967295", "4294967294"]);
        let keys: Vec<&str> = sc.frequencies().map(|(w, _)| w).collect();
        assert_eq!(
            keys,
            ["0", "2", "10", "4294967294", "b", "a", "01", "4294967295"]
        );
    }

    #[test]
    fn array_index_recognition() {
        assert_eq!(array_index("0"), Some(0));
        assert_eq!(array_index("10"), Some(10));
        assert_eq!(array_index("4294967294"), Some(4_294_967_294));
        assert_eq!(array_index("4294967295"), None);
        assert_eq!(array_index("01"), None);
        assert_eq!(array_index(""), None);
        assert_eq!(array_index("1.5"), None);
        assert_eq!(array_index("-1"), None);
        assert_eq!(array_index(" 1"), None);
        assert_eq!(array_index("99999999999999999999"), None);
    }

    #[test]
    fn the_spec_examples() {
        let sc = Spellcheck::new(["something"]);
        for typo in ["smething", "somethhing", "somehting", "sometzing"] {
            assert!(
                sc.get_corrections(typo, 1)
                    .contains(&"something".to_string()),
                "{typo}"
            );
        }
        assert!(
            sc.get_corrections("smtehing", 2)
                .contains(&"something".to_string())
        );
        assert_eq!(Spellcheck::new(["cat"]).get_corrections("cat", 1), ["cat"]);
    }

    /// The reference keeps the whole edit machinery on the prototype even though
    /// none of it touches instance state. The methods are mirrored for that
    /// reason, so they must stay exactly the free functions.
    #[test]
    fn the_edit_methods_are_the_free_functions() {
        let sc = Spellcheck::new(["cat"]);
        for word in ["", "a", "cat", "café", "a😀b"] {
            assert_eq!(sc.edits(word), crate::edits(word), "{word:?}");
            assert_eq!(sc.edits_utf16(word), crate::edits_utf16(word), "{word:?}");
            assert_eq!(
                sc.edits_with_max_distance(word, 1),
                crate::edits_with_max_distance(word, 1),
                "{word:?}"
            );
            assert_eq!(
                sc.edits_with_max_distance_utf16(word, 1),
                crate::edits_with_max_distance_utf16(word, 1),
                "{word:?}"
            );

            let mut mine = vec![vec![word.encode_utf16().collect::<Vec<u16>>()]];
            sc.edits_with_max_distance_helper(1, &mut mine);
            assert_eq!(
                mine,
                crate::edits_with_max_distance_utf16(word, 1),
                "{word:?}"
            );
        }
    }

    /// The trie is part of the reference's surface (`spellcheck.trie`) and
    /// answers prefix questions this API does not.
    #[test]
    fn the_trie_is_exposed_and_agrees_with_is_correct() {
        let sc = Spellcheck::new(["cat", "cart", "car", "dog"]);
        assert_eq!(sc.trie().get_size(), 9);
        for w in ["cat", "cart", "car", "dog", "ca", "", "x"] {
            assert_eq!(sc.trie().contains(w), sc.is_correct(w), "{w}");
        }
        let mut with_prefix = sc.trie().keys_with_prefix("ca");
        with_prefix.sort();
        assert_eq!(with_prefix, ["car", "cart", "cat"]);
    }

    /// An empty probe has only the 26 single-letter insertions as edits, so it
    /// can never correct to itself — not even when `""` is in the list.
    #[test]
    fn the_empty_probe_never_corrects_to_itself() {
        let sc = Spellcheck::new(["", "a", "b"]);
        assert!(sc.is_correct(""));
        assert_eq!(sc.get_corrections("", 1), ["a", "b"]);
        // At distance 2 a letter can be inserted and then deleted again.
        assert_eq!(sc.get_corrections("", 2), ["a", "b", ""]);
    }

    /// `Spellcheck` must be usable from several threads against one shared
    /// instance for `par_get_corrections_batch` to be sound at all. This does
    /// not need the `parallel` feature: `Arc::spawn`ing a closure that
    /// captures `Arc<Spellcheck>` only compiles if `Spellcheck: Send + Sync`,
    /// so a successful compile *is* the check, independent of whether `rayon`
    /// is in the picture.
    #[test]
    fn shared_spellcheck_is_usable_concurrently() {
        let sc = std::sync::Arc::new(Spellcheck::new(["the", "the", "the", "he", "he", "she"]));
        let expected = sc.get_corrections("th", 1);
        let handles: Vec<_> = (0..8)
            .map(|_| {
                let sc = std::sync::Arc::clone(&sc);
                let expected = expected.clone();
                std::thread::spawn(move || {
                    for _ in 0..50 {
                        assert_eq!(sc.get_corrections("th", 1), expected);
                    }
                })
            })
            .collect();
        for h in handles {
            h.join().unwrap();
        }
    }

    /// Sequential-vs-parallel parity: `par_get_corrections_batch` must return
    /// exactly what a sequential `.iter().map(get_corrections)` loop returns,
    /// element for element, for every input this crate's own sequential test
    /// suite already exercises above — not a fresh set of edge cases.
    #[cfg(feature = "parallel")]
    mod parallel_parity {
        use super::*;

        /// Runs both paths over `sc` and asserts they agree, word by word.
        fn assert_parity(sc: &Spellcheck, words: &[&str], max_distance: u32) {
            let sequential: Vec<Vec<String>> = words
                .iter()
                .map(|w| sc.get_corrections(w, max_distance))
                .collect();
            let parallel = sc.par_get_corrections_batch(words, max_distance);
            assert_eq!(
                parallel, sequential,
                "batch {words:?} at distance {max_distance} diverged"
            );
        }

        #[test]
        fn empty_input() {
            let sc = Spellcheck::new(["the", "he", "she"]);
            assert_parity(&sc, &[], 1);
            assert_parity(&sc, &[], 2);
        }

        #[test]
        fn one_word() {
            let sc = Spellcheck::new(["the", "the", "the", "he", "he", "she", "th"]);
            assert_parity(&sc, &["th"], 1);
        }

        #[test]
        fn many_words() {
            // The corpus and probes from `corrections_are_grouped_by_distance_then_frequency`
            // and `the_spec_examples`, repeated and mixed so the batch is wider
            // than any plausible thread count and includes words with zero,
            // one and several corrections.
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
            assert_parity(&sc, &words, 1);
            assert_parity(&sc, &words, 2);
        }

        /// Reuses `accented_latin_cyrillic_greek_and_cjk_round_trip` and
        /// `astral_and_emoji`'s corpora and probes: non-ASCII forces the
        /// `to_utf16` path, and the emoji case additionally splits a
        /// surrogate pair across the edit alphabet.
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
            assert_parity(&sc, &words, 1);
            assert_parity(&sc, &words, 2);
        }

        /// Reuses `prototype_shadowed_frequencies`'s corpus. Several of these
        /// words carry a `NaN` frequency (see `shadowed_frequency` above), which
        /// makes the reference's sort comparator non-transitive and its exact
        /// output a property of `comparator_sort`'s specific merge order — the
        /// sequential path's most pathological case, and the one most likely
        /// to break under a naive "just sort the merged results" parallel
        /// reimplementation. `par_get_corrections_batch` never merges
        /// anything across words, so it is unaffected, and this asserts that.
        #[test]
        fn prototype_shadowed_words() {
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
            let words = [
                "constructor",
                "toString",
                "valueOf",
                "__proto__",
                "cat",
                "constructoe",
                "toStrinh",
            ];
            assert_parity(&sc, &words, 1);
            assert_parity(&sc, &words, 2);
        }

        /// Reuses `empty_string_in_the_list_is_a_word` and
        /// `the_empty_probe_never_corrects_to_itself`'s corpora: an empty
        /// string as both a dictionary entry and a probe.
        #[test]
        fn empty_string_entries_and_probes() {
            let sc = Spellcheck::new(["", "a", "b", "cat"]);
            let words = ["", "a", "cat", "zzz"];
            assert_parity(&sc, &words, 1);
            assert_parity(&sc, &words, 2);
        }

        /// Reuses `very_long_input`'s corpus: a 500-character word, so a
        /// single item in the batch is far more expensive than the others.
        #[test]
        fn pathologically_long_word_mixed_with_short_ones() {
            let long = "a".repeat(500);
            let sc = Spellcheck::new([long.clone(), "cat".to_string()]);
            let almost_long = "a".repeat(499);
            let words = [almost_long.as_str(), "cat", "dog", long.as_str()];
            assert_parity(&sc, &words, 1);
        }

        /// `max_distance = 0` means `1`, reproducing the reference's falsiness
        /// check (`the` crate's `max_distance_zero_means_one` test) — the
        /// parallel path must not special-case this differently from the
        /// sequential one.
        #[test]
        fn max_distance_zero_means_one() {
            let sc = Spellcheck::new(["cat"]);
            assert_parity(&sc, &["ct", "cat", "xyz"], 0);
        }
    }
}
