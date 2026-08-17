//! The mutable stemming token, ported from the reference `token`.
//!
//! This is a standalone, independently parity-verified port of the reference's
//! own `Token` export — it carries the word being stemmed, the alphabet's vowel
//! set, and the named regions (`R1`, `R2`, `RV`) that Snowball rules are scoped
//! to. It is re-exported by `verbora-stemmers` for callers porting code that
//! constructs a `Token` directly, but the Snowball-family stemmers themselves
//! (French, German, Spanish, Italian, Dutch, Norwegian, Portuguese, Swedish,
//! Russian…) do **not** build on it internally: each has its own hand-rolled,
//! UTF-16-precise token type instead, because `Token`'s `Vec<char>`
//! representation is divergence **D1** in `docs/PARITY.md` and the individual
//! stemmers need the exact UTF-16 code-unit semantics `Token` only
//! approximates. See `crates/verbora-stemmers/src/pt.rs`'s own "Why not
//! `Token`" section for the worked example.
//!
//! # Representation and the UTF-16 question
//!
//! The reference indexes strings by UTF-16 *code unit*: `s[i]`, `s.length`, and
//! `s.slice(-n)` all work in code units. `verbora` stores the token as a
//! `Vec<char>` (Unicode scalar values) instead, which:
//!
//! * makes indexing O(1), matching the reference cost model (indexing a Rust `&str` by
//!   character position would be O(n) and would dominate stemming); and
//! * is **identical** to the reference for every character in the Basic Multilingual Plane,
//!   which covers every alphabet these stemmers target — Latin, Cyrillic, Greek,
//!   and their accented forms.
//!
//! The two representations differ only for non-BMP characters (astral-plane
//! symbols such as emoji), which the reference counts as two code units and Rust
//! counts as one. No stemming rule in the reference can match such a character, so
//! the difference is confined to length arithmetic on inputs that are not words
//! in any of the supported languages. This is a deliberate, documented
//! divergence rather than an oversight; see `docs/PARITY.md`.

use std::fmt;

/// A word being stemmed, plus the vowel set and region marks Snowball needs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Token {
    /// The working string, as Unicode scalar values.
    chars: Vec<char>,
    /// The input as first supplied, untouched by later rules.
    original: String,
    /// The alphabet's vowels.
    vowels: Vec<char>,
    /// Named region start offsets.
    ///
    /// A `Vec` rather than a `HashMap`: Snowball uses at most three regions, and
    /// a linear scan over three short keys beats hashing on every lookup while
    /// keeping the data contiguous.
    regions: Vec<(Box<str>, usize)>,
}

impl Token {
    /// Creates a token from `string`.
    pub fn new(string: &str) -> Self {
        Self {
            chars: string.chars().collect(),
            original: string.to_owned(),
            vowels: Vec::new(),
            regions: Vec::new(),
        }
    }

    /// Sets the vowel set, returning `self` for chaining.
    ///
    /// Mirrors `Token#usingVowels`.
    #[must_use]
    pub fn using_vowels(mut self, vowels: &str) -> Self {
        self.vowels = vowels.chars().collect();
        self
    }

    /// The current working string.
    pub fn as_string(&self) -> String {
        self.chars.iter().collect()
    }

    /// The current working string as a character slice.
    pub fn chars(&self) -> &[char] {
        &self.chars
    }

    /// The token as originally supplied.
    pub fn original(&self) -> &str {
        &self.original
    }

    /// The working string's length, in characters.
    pub fn len(&self) -> usize {
        self.chars.len()
    }

    /// Whether the working string is empty.
    pub fn is_empty(&self) -> bool {
        self.chars.is_empty()
    }

    /// Replaces the working string wholesale.
    pub fn set_string(&mut self, s: &str) {
        self.chars.clear();
        self.chars.extend(s.chars());
    }

    // -- regions ------------------------------------------------------------

    /// Marks a region as starting at `index`.
    ///
    /// Mirrors `Token#markRegion` in its numeric form.
    pub fn mark_region(&mut self, region: &str, index: usize) -> &mut Self {
        match self.regions.iter_mut().find(|(name, _)| &**name == region) {
            Some(slot) => slot.1 = index,
            None => self.regions.push((region.into(), index)),
        }
        self
    }

    /// Marks a region using a closure that computes the start index.
    ///
    /// Mirrors `Token#markRegion`'s callback form.
    pub fn mark_region_with<F>(&mut self, region: &str, f: F) -> &mut Self
    where
        F: FnOnce(&Self) -> usize,
    {
        let index = f(self);
        self.mark_region(region, index)
    }

    /// The start index of `region`.
    ///
    /// Returns 0 for an unmarked region, matching the reference's
    /// `this.regions[region] || 0`.
    pub fn region(&self, region: &str) -> usize {
        self.regions
            .iter()
            .find(|(name, _)| &**name == region)
            .map_or(0, |(_, idx)| *idx)
    }

    // -- vowels -------------------------------------------------------------

    /// Whether the character at `index` is a vowel.
    ///
    /// Out-of-range indices are not vowels, matching the reference, where
    /// `this.string[index]` is `undefined` and `indexOf(undefined)` is `-1`.
    pub fn has_vowel_at_index(&self, index: usize) -> bool {
        self.chars
            .get(index)
            .is_some_and(|c| self.vowels.contains(c))
    }

    /// The index of the next vowel at or after `start`, or the string length.
    ///
    /// The reference guards with `start >= 0 && start < length`; the `>= 0` half is
    /// unrepresentable for `usize`, so only the upper bound is checked here.
    pub fn next_vowel_index(&self, start: usize) -> usize {
        let mut index = if start < self.chars.len() {
            start
        } else {
            self.chars.len()
        };
        while index < self.chars.len() && !self.has_vowel_at_index(index) {
            index += 1;
        }
        index
    }

    /// The index of the next consonant at or after `start`, or the string length.
    pub fn next_consonant_index(&self, start: usize) -> usize {
        let mut index = if start < self.chars.len() {
            start
        } else {
            self.chars.len()
        };
        while index < self.chars.len() && self.has_vowel_at_index(index) {
            index += 1;
        }
        index
    }

    // -- suffixes -----------------------------------------------------------

    /// Whether the working string ends with `suffix`.
    ///
    /// # The empty-suffix quirk
    ///
    /// The reference computes `this.string.slice(-suffix.length) === suffix`. For an
    /// empty suffix that is `slice(-0)`, and since `-0 === 0` this yields the
    /// *entire* string rather than an empty one — so `hasSuffix('')` is true only
    /// when the token itself is empty. That inversion is reproduced here; a
    /// naive `ends_with("")` would return `true` unconditionally and silently
    /// change which stemming rules fire.
    pub fn has_suffix(&self, suffix: &str) -> bool {
        if suffix.is_empty() {
            return self.chars.is_empty();
        }
        let suffix_chars: Vec<char> = suffix.chars().collect();
        if suffix_chars.len() > self.chars.len() {
            return false;
        }
        self.chars[self.chars.len() - suffix_chars.len()..] == suffix_chars[..]
    }

    /// Whether the working string ends with `suffix` and that suffix begins at or
    /// after the start of `region`.
    pub fn has_suffix_in_region(&self, suffix: &str, region: &str) -> bool {
        let region_start = self.region(region);
        let suffix_len = suffix.chars().count();
        if suffix_len > self.chars.len() {
            return false;
        }
        let suffix_start = self.chars.len() - suffix_len;
        self.has_suffix(suffix) && suffix_start >= region_start
    }

    /// Replaces the first matching suffix from `suffixes` with `replacement`,
    /// but only if the match lies within `region`.
    ///
    /// Mirrors `Token#replaceSuffixInRegion`, including its short-circuit: the
    /// first suffix that matches wins and the rest are not tried.
    pub fn replace_suffix_in_region(
        &mut self,
        suffixes: &[&str],
        replacement: &str,
        region: &str,
    ) -> &mut Self {
        for suffix in suffixes {
            if self.has_suffix_in_region(suffix, region) {
                let keep = self.chars.len() - suffix.chars().count();
                self.chars.truncate(keep);
                self.chars.extend(replacement.chars());
                return self;
            }
        }
        self
    }

    /// Replaces every occurrence of `find` with `replace`.
    ///
    /// Mirrors `Token#replaceAll`, which is implemented in the reference as
    /// `split(find).join(replace)`.
    pub fn replace_all(&mut self, find: &str, replace: &str) -> &mut Self {
        if find.is_empty() {
            return self;
        }
        let current: String = self.chars.iter().collect();
        if !current.contains(find) {
            return self; // Common case: avoid rebuilding the buffer at all.
        }
        let replaced = current.replace(find, replace);
        self.chars.clear();
        self.chars.extend(replaced.chars());
        self
    }
}

impl fmt::Display for Token {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for c in &self.chars {
            f.write_str(c.encode_utf8(&mut [0u8; 4]))?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_suffix_matches_only_empty_token() {
        // The `slice(-0)` inversion described on `has_suffix`.
        assert!(!Token::new("word").has_suffix(""));
        assert!(Token::new("").has_suffix(""));
    }

    #[test]
    fn suffix_longer_than_token_does_not_match() {
        assert!(!Token::new("ab").has_suffix("xxxab"));
    }

    #[test]
    fn suffix_matching_is_character_based() {
        let t = Token::new("époque");
        assert!(t.has_suffix("que"));
        assert!(!t.has_suffix("Que"));
    }

    #[test]
    fn unmarked_region_starts_at_zero() {
        let t = Token::new("word");
        assert_eq!(t.region("R1"), 0);
        assert!(t.has_suffix_in_region("rd", "R1"));
    }

    #[test]
    fn region_gates_suffix_replacement() {
        let mut t = Token::new("national");
        t.mark_region("R1", 6); // suffix must start at index >= 6
        t.replace_suffix_in_region(&["ational"], "ate", "R1");
        assert_eq!(t.as_string(), "national", "suffix starts at 1, outside R1");

        t.mark_region("R1", 1);
        t.replace_suffix_in_region(&["ational"], "ate", "R1");
        assert_eq!(t.as_string(), "nate");
    }

    #[test]
    fn first_matching_suffix_wins() {
        let mut t = Token::new("running");
        // Both suffixes match; "ing" is tried first and wins, leaving "runn".
        // Had "ning" won instead, the result would be "run" — so this assertion
        // is what pins the short-circuit order.
        t.replace_suffix_in_region(&["ing", "ning"], "", "R1");
        assert_eq!(t.as_string(), "runn");

        let mut t = Token::new("running");
        t.replace_suffix_in_region(&["ning", "ing"], "", "R1");
        assert_eq!(t.as_string(), "run");
    }

    #[test]
    fn vowel_scanning() {
        let t = Token::new("strength").using_vowels("aeiou");
        assert_eq!(t.next_vowel_index(0), 3);
        assert_eq!(t.next_consonant_index(3), 4);
        // Out-of-range start clamps to length.
        assert_eq!(t.next_vowel_index(99), 8);
        assert!(!t.has_vowel_at_index(99));
    }

    #[test]
    fn replace_all_matches_split_join() {
        let mut t = Token::new("banana");
        t.replace_all("an", "X");
        assert_eq!(t.as_string(), "bXXa");

        // Empty needle is a no-op rather than a panic.
        let mut t2 = Token::new("abc");
        t2.replace_all("", "X");
        assert_eq!(t2.as_string(), "abc");
    }

    #[test]
    fn original_survives_mutation() {
        let mut t = Token::new("running");
        t.replace_suffix_in_region(&["ing"], "", "R1");
        assert_eq!(t.as_string(), "runn");
        assert_eq!(t.original(), "running");
    }
}
