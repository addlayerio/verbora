//! One document: its optional key, and its term counts.
//!
//! # Representation
//!
//! A document is a `Vec<(TermId, u32)>` in first-occurrence order plus a hash
//! index from id to position. Nothing here materialises a `terms × documents`
//! matrix, and a term that appears in fifty documents is stored once in the
//! corpus [`Interner`] rather than fifty times.
//!
//! Counting during ingestion goes through a *dense* slot table indexed by term
//! id ([`Scratch`]) rather than through the document's hash index, so a
//! repeated term costs one array load instead of one hash probe. The table is
//! owned by the corpus and restored to all-zeroes after each document by
//! walking only the ids that document touched, so the reset is
//! `O(distinct terms in the document)` and never `O(corpus vocabulary)`.

use rustc_hash::FxHashMap;

use crate::interner::{Interner, TermId};

/// The largest count one term can carry in one document.
///
/// A count is a `u32` and increments **saturate** at this value. Reaching it
/// takes over four billion occurrences of a single term inside a single
/// document, i.e. a document of at least four gigabytes; the limit is stated
/// because a silent wrap would be a wrong answer, and saturation is the
/// behaviour a caller can reason about.
pub const MAX_TERM_COUNT: u32 = u32::MAX;

/// One document in a [`TfIdf`](crate::TfIdf) corpus.
///
/// A document is obtained from [`TfIdf::document`](crate::TfIdf::document) or
/// [`TfIdf::documents`](crate::TfIdf::documents); it cannot be constructed
/// independently, because its term ids are only meaningful to the corpus that
/// issued them. Its *terms* are read through
/// [`TfIdf::document_terms`](crate::TfIdf::document_terms), which has the
/// corpus term table on hand to name them.
#[derive(Debug, Clone)]
pub struct Document {
    key: Option<Box<str>>,
    entries: Vec<(TermId, u32)>,
    index: FxHashMap<TermId, u32>,
    total: u64,
}

impl Document {
    /// Assembles a document from entries already in first-occurrence order,
    /// with at most one entry per id.
    pub(crate) fn from_parts(
        key: Option<Box<str>>,
        entries: Vec<(TermId, u32)>,
        total: u64,
    ) -> Self {
        let mut index: FxHashMap<TermId, u32> =
            FxHashMap::with_capacity_and_hasher(entries.len(), rustc_hash::FxBuildHasher);
        for (position, (id, _)) in entries.iter().enumerate() {
            index.insert(
                *id,
                u32::try_from(position).expect("a document holds fewer than u32::MAX terms"),
            );
        }
        Self {
            key,
            entries,
            index,
            total,
        }
    }

    /// The caller-supplied key, if this document was added with one.
    ///
    /// Keys are opaque to the crate: nothing is parsed out of them and nothing
    /// is required of them, including uniqueness. They exist so a caller can
    /// map a corpus position back to whatever it came from.
    #[must_use]
    pub fn key(&self) -> Option<&str> {
        self.key.as_deref()
    }

    /// How many **distinct** terms this document contains.
    #[must_use]
    pub fn distinct_terms(&self) -> usize {
        self.entries.len()
    }

    /// How many term occurrences this document contains, counting repeats.
    ///
    /// This is the number of tokens that survived the analyzer, not the number
    /// of tokens in the source text: tokens removed by the stop-word list are
    /// not counted. It saturates at `u64::MAX`, which no document can reach.
    #[must_use]
    pub fn total_terms(&self) -> u64 {
        self.total
    }

    /// Whether this document contains no terms at all.
    ///
    /// True for an empty document, for one made only of whitespace or
    /// punctuation, and for one whose every token was a stop word. Such a
    /// document still occupies a corpus position and still counts towards the
    /// document total every idf divides by.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// The count stored for `id`, or `0` if this document does not contain it.
    pub(crate) fn count(&self, id: TermId) -> u32 {
        self.index
            .get(&id)
            .and_then(|position| self.entries.get(*position as usize))
            .map_or(0, |(_, count)| *count)
    }

    /// The `(id, count)` entries, in first-occurrence order.
    pub(crate) fn entries(&self) -> &[(TermId, u32)] {
        &self.entries
    }
}

/// Reusable ingest buffers, owned by the corpus so it pays for them once
/// rather than once per document.
///
/// Invariant between documents: `slots` is all zeroes and `touched` is empty.
#[derive(Debug, Clone, Default)]
pub(crate) struct Scratch {
    /// Per-term-id state for the document being built: `0` when the term has
    /// not been seen yet, else its entry position plus one.
    slots: Vec<u32>,
    /// The ids whose slot is nonzero, for the `O(touched)` reset.
    touched: Vec<u32>,
    /// Owned tokens, used only by the custom-tokenizer path.
    tokens: Vec<String>,
}

/// Accumulates one document's term counts.
struct Counter<'a> {
    interner: &'a mut Interner,
    slots: &'a mut Vec<u32>,
    touched: &'a mut Vec<u32>,
    entries: Vec<(TermId, u32)>,
    total: u64,
}

impl Counter<'_> {
    /// Records one occurrence of `term`.
    #[inline]
    fn observe(&mut self, term: &str) {
        let id = self.interner.intern(term);
        let slot = id.index();
        if slot >= self.slots.len() {
            self.slots.resize(slot + 1, 0);
        }
        let seen = self.slots[slot];
        if seen == 0 {
            let position = u32::try_from(self.entries.len())
                .expect("a document holds fewer than u32::MAX terms");
            assert!(
                position < u32::MAX,
                "a document holds fewer than u32::MAX terms"
            );
            self.entries.push((id, 1));
            self.slots[slot] = position + 1;
            self.touched.push(
                u32::try_from(slot).expect("term ids are u32 and this one came from the interner"),
            );
        } else {
            let entry = &mut self.entries[(seen - 1) as usize];
            // Saturating, not wrapping: see [`MAX_TERM_COUNT`] for the limit
            // and for why a silent wrap would be the wrong answer.
            entry.1 = entry.1.saturating_add(1);
        }
        self.total = self.total.saturating_add(1);
    }

    /// Restores the scratch invariant and produces the document.
    fn finish(self, key: Option<Box<str>>) -> Document {
        for id in self.touched.iter() {
            self.slots[*id as usize] = 0;
        }
        self.touched.clear();
        Document::from_parts(key, self.entries, self.total)
    }
}

/// Builds a document by running `analyzer` over `text`.
pub(crate) fn build_from_text(
    interner: &mut Interner,
    scratch: &mut Scratch,
    analyzer: &crate::Analyzer,
    text: &str,
    key: Option<Box<str>>,
) -> Document {
    let Scratch {
        slots,
        touched,
        tokens,
    } = scratch;
    let mut counter = Counter {
        interner,
        slots,
        touched,
        entries: Vec::new(),
        total: 0,
    };
    analyzer.for_each_term(text, tokens, |term| counter.observe(term));
    counter.finish(key)
}

/// Builds a document from terms that have already been analyzed.
pub(crate) fn build_from_terms<I, S>(
    interner: &mut Interner,
    scratch: &mut Scratch,
    terms: I,
    key: Option<Box<str>>,
) -> Document
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let Scratch { slots, touched, .. } = scratch;
    let mut counter = Counter {
        interner,
        slots,
        touched,
        entries: Vec::new(),
        total: 0,
    };
    for term in terms {
        counter.observe(term.as_ref());
    }
    counter.finish(key)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Analyzer;

    fn ingest(text: &str) -> (Document, Interner) {
        let mut interner = Interner::default();
        let mut scratch = Scratch::default();
        let doc = build_from_text(
            &mut interner,
            &mut scratch,
            &Analyzer::new(),
            text,
            Some(Box::from("k")),
        );
        (doc, interner)
    }

    fn named(doc: &Document, interner: &Interner) -> Vec<(String, u32)> {
        doc.entries()
            .iter()
            .map(|(id, count)| (interner.name_of(*id).to_owned(), *count))
            .collect()
    }

    #[test]
    fn counts_repeats_and_keeps_first_occurrence_order() {
        let (doc, interner) = ingest("Node ruby node NODE ruby");
        assert_eq!(
            named(&doc, &interner),
            [("node".to_owned(), 3), ("ruby".to_owned(), 2)]
        );
        assert_eq!(doc.distinct_terms(), 2);
        assert_eq!(doc.total_terms(), 5);
        assert!(!doc.is_empty());
        assert_eq!(doc.key(), Some("k"));
    }

    #[test]
    fn an_absent_term_counts_zero_and_never_appears_as_an_entry() {
        let (doc, mut interner) = ingest("node");
        let absent = interner.intern("ruby");
        assert_eq!(doc.count(absent), 0);
        assert_eq!(doc.distinct_terms(), 1);
    }

    #[test]
    fn a_document_with_no_terms_is_empty_but_real() {
        let (doc, _) = ingest("  ,,, --- ");
        assert!(doc.is_empty());
        assert_eq!(doc.distinct_terms(), 0);
        assert_eq!(doc.total_terms(), 0);
        assert_eq!(doc.key(), Some("k"));
    }

    #[test]
    fn scratch_is_restored_between_documents() {
        let mut interner = Interner::default();
        let mut scratch = Scratch::default();
        let analyzer = Analyzer::new();
        for text in ["a b a", "b c", "", "don't don't", "a"] {
            let _ = build_from_text(&mut interner, &mut scratch, &analyzer, text, None);
            assert!(scratch.touched.is_empty(), "touched dirty after {text:?}");
            assert!(
                scratch.slots.iter().all(|s| *s == 0),
                "slots dirty after {text:?}"
            );
        }
        // …and a term repeated across documents still counts from one each time.
        let doc = build_from_text(&mut interner, &mut scratch, &analyzer, "a a b", None);
        assert_eq!(
            named(&doc, &interner),
            [("a".to_owned(), 2), ("b".to_owned(), 1)]
        );
    }

    #[test]
    fn pre_analyzed_terms_are_used_verbatim() {
        let mut interner = Interner::default();
        let mut scratch = Scratch::default();
        let doc = build_from_terms(
            &mut interner,
            &mut scratch,
            ["Node", "node", "NODE"],
            Some(Box::from("verbatim")),
        );
        // No folding: three spellings, three terms.
        assert_eq!(
            named(&doc, &interner),
            [
                ("Node".to_owned(), 1),
                ("node".to_owned(), 1),
                ("NODE".to_owned(), 1)
            ]
        );
        assert_eq!(doc.total_terms(), 3);
    }
}
