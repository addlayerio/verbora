//! The edit generator: every string one Damerau–Levenshtein edit away from a
//! word, in the reference's exact order.
//!
//! This is the lazy primitive the whole crate is built on. `Spellcheck` never
//! materialises an edit level it does not need — at `maxDistance = 2` the second
//! level runs to tens of thousands of strings, and the reference builds all of
//! them as a reference array before filtering.

use rustc_hash::FxHashSet;

use crate::units::{EditUnit, to_utf16};

/// The insertion and replacement alphabet, verbatim from the reference.
///
/// It is **lowercase ASCII only**. That is why `new Spellcheck(['Cat'])` cannot
/// correct `'cat'` — no edit can produce a capital `C` — while it *can* correct
/// `'Cta'`, because transposition moves existing characters rather than
/// introducing new ones.
pub const ALPHABET: &[u8; 26] = b"abcdefghijklmnopqrstuvwxyz";

/// Which of the four edit kinds the generator is due to emit next.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Stage {
    Delete,
    Transpose,
    /// Replaces and inserts, interleaved: `replace_a, insert_a, replace_b, …`.
    ReplaceInsert,
}

/// A lazy, deduplicated iterator over the strings one edit away from a word.
///
/// The generation order is observable — it survives into `getCorrections`
/// through a *stable* sort — so it is reproduced exactly. For each position
/// `i` in `0..=len`:
///
/// 1. delete the unit at `i - 1` (skipped at `i == 0`),
/// 2. transpose the units at `i - 1` and `i` (skipped at `i == 0`),
/// 3. for each letter `a`–`z`: replace the unit at `i - 1` (skipped at
///    `i == 0`), then insert the letter before `i`.
///
/// Duplicates are dropped keeping the **first** occurrence, matching the
/// reference's `filter((v, i, a) => a.indexOf(v) === i)`. The reference does
/// that in O(n²); this does it in O(n) with a hash set, which is the same
/// answer.
///
/// # A quirk worth naming
///
/// At `i == len` the transpose degenerates: `word.slice(len, len + 1)` and
/// `word.slice(len + 1, len)` are both empty, so it reconstructs the word
/// itself. **Every word of length >= 1 is therefore one of its own edits**, and
/// `getCorrections('cat')` on a list containing `'cat'` returns `'cat'` first.
/// A port that "cleans up" the degenerate case loses that.
///
/// # Examples
///
/// ```
/// use verbora_spellcheck::Edits;
///
/// // The empty word has only the 26 single-letter insertions.
/// let mut it = Edits::new(b"");
/// assert_eq!(it.next(), Some(b"a".to_vec()));
/// assert_eq!(Edits::new(b"").count(), 26);
///
/// // A one-character word can be deleted down to the empty string.
/// assert!(Edits::new(b"a").any(|e| e.is_empty()));
/// ```
pub struct Edits<'a, U: EditUnit> {
    word: &'a [U],
    /// Current split position, `0..=word.len()`.
    i: usize,
    stage: Stage,
    /// Index into [`ALPHABET`].
    k: usize,
    /// Within `Stage::ReplaceInsert`, `false` means "the replace is next".
    on_insert: bool,
    /// The candidate under construction; also what `next_edit` hands back.
    scratch: Vec<U>,
    seen: FxHashSet<Box<[U]>>,
}

impl<'a, U: EditUnit> Edits<'a, U> {
    /// Starts a generator over `word`.
    pub fn new(word: &'a [U]) -> Self {
        // The reference produces 26 + 54 * len raw candidates, and dedup removes
        // roughly a third; reserving avoids rehashing the common cases entirely.
        let capacity = 26 + 54 * word.len();
        Self {
            word,
            i: 0,
            stage: Stage::Delete,
            k: 0,
            on_insert: false,
            scratch: Vec::with_capacity(word.len() + 1),
            seen: FxHashSet::with_capacity_and_hasher(capacity, rustc_hash::FxBuildHasher),
        }
    }

    /// Yields the next distinct edit, borrowed from the generator's own buffer.
    ///
    /// This is the allocation-free form: nothing is copied out per candidate, so
    /// a distance-2 search over a nine-letter word walks ~270,000 candidates
    /// with one buffer rather than 270,000 `Vec`s. [`Iterator::next`] is the
    /// owning convenience built on top of it.
    pub fn next_edit(&mut self) -> Option<&[U]> {
        loop {
            self.next_raw()?;
            if !self.seen.contains(self.scratch.as_slice()) {
                self.seen.insert(self.scratch.as_slice().into());
                break;
            }
        }
        Some(&self.scratch)
    }

    /// Restarts the generator on another word, keeping the buffers.
    ///
    /// A distance-2 search runs one generator per element of the previous
    /// level — several hundred of them — and each would otherwise allocate a
    /// fresh dedup table sized for the raw candidate count. Recycling turns
    /// that into `clear()`.
    ///
    /// # Examples
    ///
    /// ```
    /// use verbora_spellcheck::Edits;
    ///
    /// let first = Edits::new(b"ab");
    /// let second = first.restart(b"cd");
    /// assert_eq!(second.count(), 130);
    /// ```
    pub fn restart<'b>(mut self, word: &'b [U]) -> Edits<'b, U> {
        self.seen.clear();
        self.scratch.clear();
        Edits {
            word,
            i: 0,
            stage: Stage::Delete,
            k: 0,
            on_insert: false,
            scratch: self.scratch,
            seen: self.seen,
        }
    }

    /// Builds the next candidate into `scratch`, duplicates included.
    fn next_raw(&mut self) -> Option<()> {
        let len = self.word.len();
        loop {
            if self.i > len {
                return None;
            }
            match self.stage {
                Stage::Delete => {
                    self.stage = Stage::Transpose;
                    if self.i > 0 {
                        // word[..i-1] + word[i..]
                        self.build(&[Part::Range(0, self.i - 1), Part::Range(self.i, len)]);
                        return Some(());
                    }
                }
                Stage::Transpose => {
                    self.stage = Stage::ReplaceInsert;
                    self.k = 0;
                    self.on_insert = false;
                    if self.i > 0 {
                        // word[..i-1] + word[i..i+1] + word[i-1..i] + word[i+1..]
                        // At i == len the two middle slices collapse to "" and
                        // word[len-1..len], reproducing the word itself.
                        self.build(&[
                            Part::Range(0, self.i - 1),
                            Part::Range(self.i, (self.i + 1).min(len)),
                            Part::Range(self.i - 1, self.i),
                            Part::Range((self.i + 1).min(len), len),
                        ]);
                        return Some(());
                    }
                }
                Stage::ReplaceInsert => {
                    if self.k >= ALPHABET.len() {
                        self.i += 1;
                        self.stage = Stage::Delete;
                        continue;
                    }
                    if !self.on_insert {
                        self.on_insert = true;
                        if self.i > 0 {
                            // word[..i-1] + letter + word[i..]
                            self.build(&[
                                Part::Range(0, self.i - 1),
                                Part::Letter(self.k),
                                Part::Range(self.i, len),
                            ]);
                            return Some(());
                        }
                        continue;
                    }
                    let k = self.k;
                    self.k += 1;
                    self.on_insert = false;
                    // word[..i] + letter + word[i..]
                    self.build(&[
                        Part::Range(0, self.i),
                        Part::Letter(k),
                        Part::Range(self.i, len),
                    ]);
                    return Some(());
                }
            }
        }
    }

    fn build(&mut self, parts: &[Part]) {
        self.scratch.clear();
        for part in parts {
            match *part {
                Part::Range(a, b) => self.scratch.extend_from_slice(&self.word[a..b]),
                Part::Letter(k) => self.scratch.push(U::from_ascii(ALPHABET[k])),
            }
        }
    }
}

/// A piece of a candidate: a slice of the source word, or an alphabet letter.
#[derive(Clone, Copy)]
enum Part {
    Range(usize, usize),
    Letter(usize),
}

impl<U: EditUnit> Iterator for Edits<'_, U> {
    type Item = Vec<U>;

    fn next(&mut self) -> Option<Self::Item> {
        self.next_edit().map(<[U]>::to_vec)
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        // Exact counts are not known without generating, but the raw upper
        // bound is: 26 + 54 * len.
        (0, Some(26 + 54 * self.word.len()))
    }
}

/// All strings one edit away from `word`, in generation order, deduplicated.
///
/// # Divergence: lone surrogates
///
/// When `word` contains an astral character, some edits cut a surrogate pair in
/// half. The reference hands those back as strings containing an unpaired
/// surrogate; a Rust `String` cannot hold one, so this function renders the
/// broken half as `U+FFFD`. **Two distinct the reference results can therefore
/// collapse onto the same Rust string.** Use [`edits_utf16`] when that matters —
/// `Spellcheck` itself never goes through this function.
///
/// # Examples
///
/// ```
/// use verbora_spellcheck::edits;
///
/// assert_eq!(edits("").len(), 26);
/// assert_eq!(edits("a").len(), 78);
/// assert_eq!(edits("something").len(), 494);
/// // The degenerate transposition at the end of the word: 'cat' edits to itself.
/// assert!(edits("cat").contains(&"cat".to_string()));
/// ```
pub fn edits(word: &str) -> Vec<String> {
    if word.is_ascii() {
        Edits::new(word.as_bytes())
            .map(|e| u8::to_lossy_string(&e))
            .collect()
    } else {
        let units = to_utf16(word);
        Edits::new(&units)
            .map(|e| u16::to_lossy_string(&e))
            .collect()
    }
}

/// All strings one edit away from `word`, as raw UTF-16 code units.
///
/// The exact form: unlike [`edits`], nothing is lost when an edit splits a
/// surrogate pair.
///
/// # Examples
///
/// ```
/// use verbora_spellcheck::edits_utf16;
///
/// // "a😀b" is four UTF-16 code units, so it has more edits than its three
/// // characters would suggest — and 83 of them are not valid text.
/// let all = edits_utf16("a😀b");
/// assert_eq!(all.len(), 238);
/// let broken = all.iter().filter(|e| String::from_utf16(e).is_err()).count();
/// assert_eq!(broken, 83);
/// ```
pub fn edits_utf16(word: &str) -> Vec<Vec<u16>> {
    let units = to_utf16(word);
    Edits::new(&units).collect()
}

/// Every edit level from 0 up to `distance`, as raw UTF-16 code units.
///
/// Level 0 is `[word]`; level `k` is the concatenation of `edits` over *every*
/// element of level `k - 1`.
///
/// # This grows about 500x per level
///
/// Level `k` is **not** deduplicated across the previous level's elements — the
/// reference dedups only within a single `edits` call — so
/// `edits_with_max_distance_utf16("abc", 2)` returns levels of 1, 182 and 38,206
/// strings, of which only 14,352 are distinct. That duplication is observable
/// (it decides how often a word is re-examined and therefore nothing about the
/// final answer, but it *is* what the reference builds), so it is preserved.
///
/// Prefer [`Spellcheck::get_corrections`](crate::Spellcheck::get_corrections),
/// which streams the last level instead of materialising it.
///
/// # Examples
///
/// ```
/// use verbora_spellcheck::edits_with_max_distance_utf16;
///
/// let levels = edits_with_max_distance_utf16("abc", 2);
/// assert_eq!(levels.iter().map(Vec::len).collect::<Vec<_>>(), [1, 182, 38206]);
/// ```
pub fn edits_with_max_distance_utf16(word: &str, distance: u32) -> Vec<Vec<Vec<u16>>> {
    let mut levels = vec![vec![to_utf16(word)]];
    edits_with_max_distance_helper(distance, &mut levels);
    levels
}

/// Appends `distance_counter` further edit levels to `levels`, in place.
///
/// This is the reference's `editsWithMaxDistanceHelper`, which is on the
/// prototype and therefore part of its public surface. Two properties of it are
/// observable and are reproduced here rather than tidied away:
///
/// * it **mutates the array it is handed** (and returns the same array — which
///   in Rust the `&mut` signature says outright);
/// * it expands the **deepest** level present, not the word it started from, so
///   seeding it with two levels is not the same as asking for distance 2.
///
/// `distance_counter == 0` leaves `levels` untouched, matching the reference's
/// `if (distanceCounter === 0) return distance2edits`.
///
/// # Why there is no `String` form
///
/// The output of one call is the input of the next, and a level rendered as
/// `String`s has already lost the lone surrogates an astral input produces (see
/// [`edits`]). A lossy level cannot be fed back in, so offering one would be
/// offering a footgun.
///
/// # This grows about 500x per level
///
/// See [`edits_with_max_distance_utf16`]. Three levels from a one-character word
/// is already ~10<sup>6</sup> strings.
///
/// # Examples
///
/// ```
/// use verbora_spellcheck::{edits_utf16, edits_with_max_distance_helper};
///
/// // Seeded with two levels: the helper expands the *last* one.
/// let mut levels = vec![
///     vec!["ab".encode_utf16().collect::<Vec<u16>>()],
///     vec!["xy".encode_utf16().collect::<Vec<u16>>()],
/// ];
/// edits_with_max_distance_helper(1, &mut levels);
/// assert_eq!(levels.len(), 3);
/// assert_eq!(levels[2], edits_utf16("xy"));
///
/// // A level holding the same word twice concatenates its edits twice: the
/// // reference dedups only inside one `edits` call.
/// let mut levels = vec![vec![vec![b'a' as u16], vec![b'a' as u16]]];
/// edits_with_max_distance_helper(1, &mut levels);
/// assert_eq!(levels[1].len(), 2 * edits_utf16("a").len());
/// ```
pub fn edits_with_max_distance_helper(distance_counter: u32, levels: &mut Vec<Vec<Vec<u16>>>) {
    for _ in 0..distance_counter {
        let next: Vec<Vec<u16>> = match levels.last() {
            Some(words) => {
                let mut next = Vec::new();
                for w in words {
                    next.extend(Edits::new(w));
                }
                next
            }
            // The reference reads `distance2edits[-1]`, gets `undefined`, and
            // `for (const i in undefined)` iterates nothing — so an empty seed
            // grows one empty level per step rather than throwing.
            None => Vec::new(),
        };
        levels.push(next);
    }
}

/// Every edit level from 0 up to `distance`, rendered as `String`s.
///
/// Convenience over [`edits_with_max_distance_utf16`]; it carries the same
/// lone-surrogate divergence as [`edits`], and the same appetite for memory.
pub fn edits_with_max_distance(word: &str, distance: u32) -> Vec<Vec<String>> {
    edits_with_max_distance_utf16(word, distance)
        .into_iter()
        .map(|level| level.iter().map(|e| u16::to_lossy_string(e)).collect())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use verbora_distance::levenshtein::{Options, damerau_levenshtein};

    fn ascii_edits(word: &str) -> Vec<String> {
        Edits::new(word.as_bytes())
            .map(|e| String::from_utf8(e).unwrap())
            .collect()
    }

    #[test]
    fn empty_word_yields_the_alphabet() {
        let e = ascii_edits("");
        assert_eq!(e.len(), 26);
        assert_eq!(e[0], "a");
        assert_eq!(e[25], "z");
    }

    #[test]
    fn single_character() {
        let e = ascii_edits("a");
        assert_eq!(e.len(), 78);
        // i = 0 inserts come first: "aa", "ba", ... "za".
        assert_eq!(&e[..3], ["aa", "ba", "ca"]);
        // Then i = 1: the delete (empty string), the degenerate transpose ("a").
        assert_eq!(e[26], "");
        assert_eq!(e[27], "a");
    }

    #[test]
    fn deduplication_keeps_the_first_occurrence() {
        let e = ascii_edits("ab");
        assert_eq!(e.len(), 130);
        // "aab" is generated at i = 0 (insert 'a') and again at i = 1 (replace
        // 'a' with 'a'); only the first survives.
        assert_eq!(e[0], "aab");
        assert_eq!(e.iter().filter(|s| *s == "aab").count(), 1);
    }

    #[test]
    fn uppercase_and_punctuation_are_preserved_but_never_introduced() {
        let e = ascii_edits("A");
        assert!(e.contains(&"A".to_string()));
        assert!(!e.iter().any(|s| s.contains('B')));
        let e = ascii_edits("-");
        assert!(e.contains(&"-".to_string()));
        assert!(e.contains(&"a-".to_string()));
    }

    #[test]
    fn digits_and_accented_latin() {
        // 26 + 54 * 3 = 188 raw, and *none* dedup away: a replacement letter can
        // never coincide with a digit, whereas "abc" loses six that way.
        assert_eq!(ascii_edits("123").len(), 188);
        assert_eq!(ascii_edits("abc").len(), 182);
        // 'é' is one UTF-16 unit, so "café" behaves like a four-unit word — but
        // its three ASCII letters still dedup against the alphabet.
        assert_eq!(edits("café").len(), 236);
        assert!(edits("café").contains(&"café".to_string()));
    }

    #[test]
    fn cyrillic_greek_and_cjk_count_as_one_unit_each() {
        // All BMP: one code unit per character, so 26 + 54 * n raw candidates.
        // Nothing dedups against the a–z alphabet, so only repeated characters
        // within the word itself collapse — 'Ελλάδα' has 'λλ' and two 'α'.
        assert_eq!(edits_utf16("Москва").len(), 26 + 54 * 6);
        assert_eq!(edits_utf16("Ελλάδα").len(), 348);
        assert_eq!(edits_utf16("日本語").len(), 26 + 54 * 3);
        // The same words written in ASCII lose candidates to the alphabet.
        assert_eq!(edits_utf16("abcdef").len(), 338);
    }

    #[test]
    fn astral_characters_count_as_two_units() {
        // '😀' is two code units, so "😀" behaves like a two-character word.
        assert_eq!(edits_utf16("😀").len(), 134);
        assert_eq!(edits_utf16("a😀b").len(), 238);
    }

    #[test]
    fn very_long_input_matches_the_raw_bound() {
        let long = "a".repeat(500);
        // Heavy duplication (deleting any 'a' gives the same string), so the
        // deduplicated count is far below the 26 + 54 * 500 raw bound.
        let e = ascii_edits(&long);
        assert!(e.len() < 26 + 54 * 500);
        assert!(e.contains(&long));
    }

    /// Cross-check against the PARITY_VERIFIED distance crate: everything the
    /// generator emits really is one restricted Damerau–Levenshtein edit away
    /// (or zero, for the degenerate transposition that reproduces the word).
    #[test]
    fn every_edit_is_within_damerau_levenshtein_distance_one() {
        let options = Options {
            restricted: true,
            ..Options::default()
        };
        for word in ["", "a", "ab", "cat", "something", "café"] {
            for candidate in edits(word) {
                let d = damerau_levenshtein(word, &candidate, &options);
                assert!(d <= 1.0, "{word:?} -> {candidate:?} is {d} edits away");
            }
        }
    }

    #[test]
    fn levels_concatenate_without_cross_element_dedup() {
        let levels = edits_with_max_distance_utf16("ab", 2);
        assert_eq!(levels[0].len(), 1);
        assert_eq!(levels[1].len(), 130);
        assert_eq!(levels[2].len(), 20740);
        let unique: FxHashSet<&Vec<u16>> = levels[2].iter().collect();
        assert_eq!(unique.len(), 7154);
    }

    #[test]
    fn distance_zero_returns_only_the_seed() {
        let levels = edits_with_max_distance("abc", 0);
        assert_eq!(levels, vec![vec!["abc".to_string()]]);
    }

    fn u(s: &str) -> Vec<u16> {
        s.encode_utf16().collect()
    }

    #[test]
    fn helper_appends_to_the_deepest_level_not_the_first() {
        let mut levels = vec![vec![u("ab")], vec![u("xy")]];
        edits_with_max_distance_helper(1, &mut levels);
        assert_eq!(levels.len(), 3);
        assert_eq!(levels[0], vec![u("ab")]);
        assert_eq!(levels[2], edits_utf16("xy"));
        // Emphatically not the same as asking for two levels from "ab".
        assert_ne!(levels[2], edits_with_max_distance_utf16("ab", 2)[2]);
    }

    #[test]
    fn helper_with_zero_leaves_the_seed_alone() {
        let mut levels = vec![vec![u("ab")]];
        edits_with_max_distance_helper(0, &mut levels);
        assert_eq!(levels, vec![vec![u("ab")]]);

        // And on an empty seed the early return happens *before* a level is
        // appended, so nothing is added at all.
        let mut empty: Vec<Vec<Vec<u16>>> = Vec::new();
        edits_with_max_distance_helper(0, &mut empty);
        assert!(empty.is_empty());
    }

    /// `distance2edits[-1]` is `undefined` in the reference and `for..in` over
    /// it iterates nothing, so this grows empty levels rather than throwing. The
    /// naive port indexes `len - 1` and panics.
    #[test]
    fn helper_on_an_empty_seed_grows_empty_levels() {
        let mut levels: Vec<Vec<Vec<u16>>> = Vec::new();
        edits_with_max_distance_helper(3, &mut levels);
        assert_eq!(levels.len(), 3);
        assert!(levels.iter().all(Vec::is_empty));
    }

    #[test]
    fn helper_repeats_a_word_that_appears_twice_in_a_level() {
        let mut levels = vec![vec![u("a"), u("a")]];
        edits_with_max_distance_helper(1, &mut levels);
        let one = edits_utf16("a");
        assert_eq!(levels[1].len(), 2 * one.len());
        assert_eq!(levels[1][..one.len()], one[..]);
        assert_eq!(levels[1][one.len()..], one[..]);
    }

    #[test]
    fn helper_is_what_edits_with_max_distance_is_built_from() {
        for word in ["", "a", "ab", "café", "a😀b"] {
            let mut levels = vec![vec![to_utf16(word)]];
            edits_with_max_distance_helper(2, &mut levels);
            assert_eq!(levels, edits_with_max_distance_utf16(word, 2), "{word:?}");
        }
    }
}
