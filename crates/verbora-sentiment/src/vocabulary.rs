//! The word → polarity tables, and how they are loaded, projected and stemmed.
//!
//! # What the reference actually stores
//!
//! `SentimentAnalyzer`'s constructor copies one of fourteen JSON objects and
//! then rewrites every value in place:
//!
//! ```text
//! senticon:  vocabulary[w] = vocabulary[w].pol        // "0.813"  — a STRING
//! pattern:   vocabulary[w] = vocabulary[w].polarity   // "-0.30"  — a STRING
//! afinn:     left alone                               // 3        — a NUMBER
//! ```
//!
//! Those types are observable: `analyzer.vocabulary['good']` is the string
//! `"0.813"` under senticon and the number `3` under AFINN, and the conversion
//! happens only in `negator * value` when a sentence is scored. [`Polarity`]
//! keeps the distinction rather than collapsing everything to `f64`.
//!
//! # Insertion order is load-bearing
//!
//! When a stemmer is supplied the whole table is rebuilt:
//!
//! ```text
//! for (const token in this.vocabulary) vocaStemmed[stemmer.stem(token)] = this.vocabulary[token]
//! ```
//!
//! Colliding stems therefore resolve **last-wins in the JSON-file order**. For
//! English AFINN with the Porter stemmer that is 3382 keys collapsing to 1967,
//! with 1415 collisions of which 109 change the stored polarity — `affect`(-1)
//! loses to `affection`(3), `arrested`(-3) loses to `arrests`(-2). A `HashMap`
//! picks a different winner for all 109 and a `BTreeMap` picks a third set, so
//! [`Vocabulary`] keeps its entries in a `Vec` in insertion order and the
//! hash map only maps a key to its slot.
//!
//! Re-assigning an existing key keeps its original position, exactly as
//! The reference object assignment does; only the value is replaced.

use std::borrow::Cow;
use std::sync::OnceLock;

use rustc_hash::FxHashMap;

use crate::data;
use crate::stemmer::Stemmer;
use crate::{Language, VocabularyKind};

/// A polarity, in the reference type the reference leaves it as.
///
/// See the module documentation: AFINN values stay numbers, senticon and
/// pattern polarities stay strings.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Polarity {
    /// An AFINN score. Every shipped value is an integer in `-5..=5`.
    Number(f64),
    /// A senticon `.pol` or pattern `.polarity`, as written in the source file.
    Text(&'static str),
}

impl Polarity {
    /// The value the reference's `negator * polarity` would produce.
    ///
    /// For [`Polarity::Text`] this is `ToNumber` on a plain decimal, which
    /// the reference and Rust's `str::parse` both round correctly and therefore
    /// agree on bit for bit. A value that is not a number at all yields `NaN`,
    /// as `ToNumber` does — no shipped file contains one.
    #[must_use]
    pub fn as_f64(self) -> f64 {
        match self {
            Self::Number(n) => n,
            Self::Text(s) => s.parse().unwrap_or(f64::NAN),
        }
    }

    /// The text a reference caller would see, for [`Polarity::Text`].
    #[must_use]
    pub fn as_str(self) -> Option<&'static str> {
        match self {
            Self::Text(s) => Some(s),
            Self::Number(_) => None,
        }
    }
}

impl std::fmt::Display for Polarity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Number(n) => write!(f, "{n}"),
            Self::Text(s) => f.write_str(s),
        }
    }
}

/// One `key -> polarity` pair.
///
/// `raw` always points into the embedded blob — stemming rewrites keys, never
/// values — so a stemmed table still answers `vocabulary[word]` with the source
/// file's own text.
#[derive(Debug, Clone)]
struct Entry {
    key: Cow<'static, str>,
    raw: &'static str,
    value: f64,
}

/// A word → polarity table: one of the fourteen shipped vocabularies, or the
/// stemmed rebuild of one.
///
/// # Memory
///
/// Keys of a shipped table are `&'static str` slices of the embedded blob, so
/// the table itself is a `Vec` of fat pointers plus a hash map of the same — no
/// string data is copied. A stemmed table owns the stems the algorithm changed
/// and borrows the ones it did not, and stores each changed key twice (once for
/// ordered iteration, once as the map's key). For the largest vocabulary,
/// English senticon, that is under a megabyte and it is built at most once per
/// analyzer.
#[derive(Debug, Clone)]
pub struct Vocabulary {
    kind: Option<VocabularyKind>,
    entries: Vec<Entry>,
    index: FxHashMap<Cow<'static, str>, u32>,
}

impl Vocabulary {
    /// An empty table with no family.
    ///
    /// Reachable from the reference: `new SentimentAnalyzer('__proto__', undefined,
    /// 'afinn')` succeeds with a zero-entry vocabulary because `'__proto__'`
    /// resolves on the prototype chain, and `Object.assign({}, undefined)` is
    /// `{}`.
    pub(crate) fn empty(kind: Option<VocabularyKind>) -> Self {
        Self {
            kind,
            entries: Vec::new(),
            index: FxHashMap::default(),
        }
    }

    /// `vocabulary[key] = value`, with the reference's own semantics: a new key is
    /// appended, an existing key keeps its position and takes the new value.
    fn set(&mut self, key: Cow<'static, str>, raw: &'static str, value: f64) {
        match self.index.get(&key) {
            Some(&slot) => {
                if let Some(entry) = self.entries.get_mut(slot as usize) {
                    entry.raw = raw;
                    entry.value = value;
                }
            }
            None => {
                let slot = u32::try_from(self.entries.len()).expect("vocabulary fits in u32");
                self.index.insert(key.clone(), slot);
                self.entries.push(Entry { key, raw, value });
            }
        }
    }

    /// Which family this table belongs to, or `None` for a degenerate empty one.
    #[must_use]
    pub fn kind(&self) -> Option<VocabularyKind> {
        self.kind
    }

    /// Number of entries — `Object.keys(analyzer.vocabulary).length`.
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the table has no entries.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// `vocabulary[word]`, in the reference type the reference stores.
    ///
    /// The lookup is exact: The reference never normalises its keys, so the 1234
    /// capitalised German pattern entries and the 5960 space-containing English
    /// senticon entries are unreachable from a lowercased single token — which
    /// is what `getSentiment` looks them up with.
    #[must_use]
    pub fn get(&self, word: &str) -> Option<Polarity> {
        let entry = self.entry(word)?;
        Some(if self.values_are_text() {
            Polarity::Text(entry.raw)
        } else {
            Polarity::Number(entry.value)
        })
    }

    /// The numeric polarity of `word`, skipping the [`Polarity`] shaping.
    ///
    /// This is the scoring hot path: the decimal text was parsed once when the
    /// table was built, so a token costs one hash and no allocation.
    #[must_use]
    pub fn polarity(&self, word: &str) -> Option<f64> {
        Some(self.entry(word)?.value)
    }

    fn entry(&self, word: &str) -> Option<&Entry> {
        let slot = *self.index.get(word)?;
        self.entries.get(slot as usize)
    }

    fn values_are_text(&self) -> bool {
        matches!(
            self.kind,
            Some(VocabularyKind::Senticon | VocabularyKind::Pattern)
        )
    }

    /// The keys in the reference enumeration order, which for every shipped table
    /// is the source file's own order.
    pub fn keys(&self) -> impl ExactSizeIterator<Item = &str> {
        self.entries.iter().map(|e| e.key.as_ref())
    }

    /// The entries in the reference enumeration order.
    pub fn iter(&self) -> impl ExactSizeIterator<Item = (&str, Polarity)> {
        let text = self.values_are_text();
        self.entries.iter().map(move |e| {
            (
                e.key.as_ref(),
                if text {
                    Polarity::Text(e.raw)
                } else {
                    Polarity::Number(e.value)
                },
            )
        })
    }

    /// The shipped table for `(kind, language)`, or `None` if the reference's
    /// `languageFiles` has no such pair.
    ///
    /// Each table is decoded at most once per process and shared thereafter;
    /// The reference rebuilds its copy on every construction.
    #[must_use]
    pub fn shared(kind: VocabularyKind, language: &str) -> Option<&'static Self> {
        Some(load(source_index(kind, language)?))
    }

    /// The shipped table for a [`Language`] known to pair with `kind`.
    #[must_use]
    pub fn shared_for(kind: VocabularyKind, language: Language) -> Option<&'static Self> {
        Self::shared(kind, language.as_str())
    }

    /// This table rebuilt through `stemmer`, the way the constructor does.
    ///
    /// Colliding stems resolve last-wins in this table's iteration order; see
    /// the module documentation for why that is not negotiable.
    #[must_use]
    pub fn stemmed<S: Stemmer + ?Sized>(&'static self, stemmer: &S) -> Self {
        let mut out = Self {
            kind: self.kind,
            entries: Vec::with_capacity(self.entries.len()),
            index: FxHashMap::with_capacity_and_hasher(
                self.entries.len(),
                rustc_hash::FxBuildHasher,
            ),
        };
        for entry in &self.entries {
            // A shipped table's keys are `&'static str`, so a stemmer that
            // leaves a word alone hands back a `'static` borrow and this whole
            // pass allocates only for the words it actually changed.
            let key: Cow<'static, str> = match &entry.key {
                Cow::Borrowed(s) => stemmer.stem(s),
                Cow::Owned(s) => Cow::Owned(stemmer.stem(s).into_owned()),
            };
            out.set(key, entry.raw, entry.value);
        }
        out
    }
}

/// The row of `languageFiles` for `(kind, language)`.
pub(crate) fn source_index(kind: VocabularyKind, language: &str) -> Option<usize> {
    data::SOURCES
        .iter()
        .position(|s| s.kind == kind && s.language == language)
}

/// Every table is decoded lazily and kept forever: The reference re-copies (and
/// re-stems) its vocabulary on every construction, which the spec measured at
/// 8.6 ms cold and 88 ms with a stemmer for English senticon.
static CACHE: [OnceLock<Vocabulary>; data::SOURCE_COUNT] =
    [const { OnceLock::new() }; data::SOURCE_COUNT];

/// Decodes one blob: `key \0 polarity \0`, entries in source order.
pub(crate) fn load(index: usize) -> &'static Vocabulary {
    let slot = CACHE.get(index).expect("source index is in range");
    slot.get_or_init(|| {
        let source = data::SOURCES.get(index).expect("source index is in range");
        let mut voca = Vocabulary {
            kind: Some(source.kind),
            entries: Vec::with_capacity(source.entries),
            index: FxHashMap::with_capacity_and_hasher(source.entries, rustc_hash::FxBuildHasher),
        };
        let Some(blob) = source.blob else {
            return voca;
        };
        // The generator proves the blob is UTF-8, NUL-terminated and has an even
        // number of fields on every run, so a malformed one is a build bug.
        let text = std::str::from_utf8(blob).expect("generated blob is valid UTF-8");
        let mut fields = text.split_terminator('\0');
        while let Some(key) = fields.next() {
            let raw = fields.next().expect("generated blob has paired fields");
            let value = raw.parse().expect("generated polarity is a plain decimal");
            voca.set(Cow::Borrowed(key), raw, value);
        }
        debug_assert_eq!(voca.len(), source.entries);
        voca
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn afinn_values_are_numbers_and_senticon_values_are_text() {
        let afinn = Vocabulary::shared(VocabularyKind::Afinn, "English").unwrap();
        assert_eq!(afinn.get("good"), Some(Polarity::Number(3.0)));
        assert_eq!(afinn.get("good").unwrap().as_str(), None);

        let senticon = Vocabulary::shared(VocabularyKind::Senticon, "English").unwrap();
        assert_eq!(senticon.get("good"), Some(Polarity::Text("0.813")));
        assert_eq!(senticon.get("good").unwrap().as_f64(), 0.813);
    }

    #[test]
    fn tables_are_shared_not_rebuilt() {
        let a = Vocabulary::shared(VocabularyKind::Pattern, "Dutch").unwrap();
        let b = Vocabulary::shared(VocabularyKind::Pattern, "Dutch").unwrap();
        assert!(std::ptr::eq(a, b));
    }

    #[test]
    fn every_shipped_table_decodes_to_its_recorded_size() {
        for (i, source) in data::SOURCES.iter().enumerate() {
            let voca = load(i);
            assert_eq!(
                voca.len(),
                source.entries,
                "{:?}/{}",
                source.kind,
                source.language
            );
            assert_eq!(voca.keys().len(), source.entries);
        }
    }

    #[test]
    fn keys_are_in_source_file_order() {
        let afinn = Vocabulary::shared(VocabularyKind::Afinn, "English").unwrap();
        let first: Vec<&str> = afinn.keys().take(4).collect();
        assert_eq!(first, ["abandon", "abandoned", "abandons", "abducted"]);
    }

    #[test]
    fn unknown_pairs_have_no_table() {
        assert!(Vocabulary::shared(VocabularyKind::Afinn, "Dutch").is_none());
        assert!(Vocabulary::shared(VocabularyKind::Pattern, "Basque").is_none());
        assert!(Vocabulary::shared(VocabularyKind::AfinnFinancialMarketNews, "English").is_some());
    }

    #[test]
    fn financial_market_news_is_empty_by_construction() {
        let voca = Vocabulary::shared(VocabularyKind::AfinnFinancialMarketNews, "English").unwrap();
        assert!(voca.is_empty());
        assert_eq!(voca.get("bankruptcy"), None);
    }

    #[test]
    fn stem_collisions_resolve_last_wins_in_file_order() {
        let base = Vocabulary::shared(VocabularyKind::Afinn, "English").unwrap();
        let stemmed = base.stemmed(&verbora_stemmers::PorterStemmer::new());
        assert_eq!(base.len(), 3382);
        assert_eq!(stemmed.len(), 1967);

        // `affection`(3) and `arrested`(-3) both collide, and both are followed
        // in file order by another key with the same stem. Last one wins.
        assert_eq!(base.polarity("affection"), Some(3.0));
        assert_eq!(stemmed.polarity("affect"), Some(3.0));
        assert_eq!(base.polarity("arrested"), Some(-3.0));
        assert_eq!(base.polarity("arrests"), Some(-2.0));
        assert_eq!(stemmed.polarity("arrest"), Some(-2.0));

        // Values still point at the source file's own text, so a stemmed
        // senticon table still answers with strings.
        let senticon = Vocabulary::shared(VocabularyKind::Senticon, "English")
            .unwrap()
            .stemmed(&verbora_stemmers::PorterStemmer::new());
        assert!(matches!(senticon.get("good"), Some(Polarity::Text(_))));
    }

    #[test]
    fn the_identity_stemmer_rebuilds_an_identical_table() {
        let base = Vocabulary::shared(VocabularyKind::Pattern, "Italian").unwrap();
        let same = base.stemmed(&crate::NoStemmer);
        assert_eq!(same.len(), base.len());
        assert!(base.keys().eq(same.keys()));
    }

    #[test]
    fn assignment_keeps_the_original_position() {
        let mut v = Vocabulary::empty(Some(VocabularyKind::Afinn));
        v.set(Cow::Borrowed("a"), "1", 1.0);
        v.set(Cow::Borrowed("b"), "2", 2.0);
        v.set(Cow::Borrowed("a"), "9", 9.0);
        assert_eq!(v.keys().collect::<Vec<_>>(), ["a", "b"]);
        assert_eq!(v.polarity("a"), Some(9.0));
    }
}
