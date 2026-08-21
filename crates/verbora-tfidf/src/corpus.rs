//! The corpus itself: ingestion, statistics, ranking and persistence.

use std::ops::Range;
use std::path::Path;

use rustc_hash::FxHashSet;

use crate::analyzer::{Analyzer, CaseFold};
use crate::document::{Document, Scratch, build_from_terms, build_from_text};
use crate::interner::{Interner, TermId};
use crate::log::natural_log;
use crate::persist::{AnalyzerRecord, Artifact, DocumentRecord, ExportError, RestoreError};

/// One term of one document, scored.
///
/// Produced by [`TfIdf::list_terms`].
#[derive(Debug, Clone, PartialEq)]
pub struct TermScore {
    /// The term, as the corpus stores it — i.e. after the analyzer's fold.
    pub term: String,
    /// How many times it occurs in the document.
    pub count: u32,
    /// Its inverse document frequency across the whole corpus.
    pub idf: f64,
    /// `count * idf`, the term's contribution to a single-term query.
    pub tfidf: f64,
}

/// One document of a ranking.
///
/// Produced by [`TfIdf::rank`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DocumentScore {
    /// The document's position in the corpus.
    pub document: usize,
    /// Its score for the query, as [`TfIdf::tfidf`] would compute it.
    pub score: f64,
}

/// A TF-IDF corpus: a growable list of documents, plus the statistics over them.
///
/// # The model
///
/// A corpus holds `n` documents. Each document is a multiset of **terms**, and
/// a term is whatever this corpus's [`Analyzer`] produces — see that type for
/// the pipeline. Nothing is global: two `TfIdf` values in one program share no
/// tokenizer, no stop-word list and no cache, so neither can change the other's
/// answers.
///
/// Three quantities are defined over that, and every public number is one of
/// them or a sum of them:
///
/// | Quantity | Definition |
/// |---|---|
/// | `count(t, d)` | occurrences of term `t` in document `d`, `0` if absent |
/// | `df(t)` | how many documents have `count(t, ·) >= 1` |
/// | `idf(t)` | `1 + ln(n / (1 + df(t)))`, for `n >= 1` |
///
/// and the score of a query `q` against document `d` is
///
/// ```text
/// tfidf(q, d) = Σ  count(t, d) · idf(t)
///              t∈q
/// ```
///
/// summed over the query's terms **in order, with multiplicity** — a query that
/// mentions a term twice counts it twice.
///
/// # Floating point is part of the contract
///
/// * The logarithm is [`natural_log`], not `f64::ln`: it is specified, so the
///   same corpus and query yield the same bits on every platform. See that
///   function for why, and for its error bound.
/// * The sum is accumulated **strictly left to right** over the query's terms,
///   starting from `0.0`. Floating-point addition is not associative, so the
///   order is specified rather than left to the implementation.
/// * **No score is ever `NaN` or infinite.** `n >= 1` and `0 <= df <= n` make
///   the logarithm's argument positive and finite, so `idf` lies in
///   `[1 + ln(n/(n+1)), 1 + ln n]` — strictly positive, always finite. Each
///   term contributes at most `u32::MAX · (1 + ln n)`, which is under `1.6e11`
///   for any corpus that fits in memory, so reaching `f64::MAX` would take on
///   the order of `1e297` query terms.
/// * The empty query scores `0.0`, exactly.
///
/// # Edge cases, stated
///
/// | Situation | Answer |
/// |---|---|
/// | empty corpus | [`idf`](Self::idf) is `None`; every per-document query is `None`; [`rank`](Self::rank) and [`tfidfs`](Self::tfidfs) are empty |
/// | term in no document | `df` is `0`, `idf` is `1 + ln n`, `count` is `0`, so it contributes `0.0` |
/// | document with no terms | scores `0.0` for every query, and still counts towards `n` |
/// | document index out of range | `None` — never a sentinel score |
///
/// # Limits
///
/// Three, all of them consequences of the integer widths the representation is
/// built on. None is reachable by a corpus that fits in memory, and each is
/// checked rather than wrapped, because a wrapped id would alias two different
/// terms and a wrapped count would be a wrong answer that nothing could detect:
///
/// | Limit | What happens at it |
/// |---|---|
/// | `u32::MAX - 1` distinct terms in one corpus | ingestion panics (over 34 GiB of term text) |
/// | `u32::MAX` distinct terms in one document | ingestion panics |
/// | [`MAX_TERM_COUNT`](crate::MAX_TERM_COUNT) occurrences of one term in one document | the count saturates |
///
/// # What a panic during ingestion leaves behind
///
/// Ingestion runs code the corpus does not own — the iterator handed to
/// [`add_terms`](Self::add_terms), a [`Tokenize`](crate::Tokenize) installed on
/// the [`Analyzer`] — and either may panic, as may the two limits above. A
/// caller who catches such an unwind gets a corpus that is **incomplete but
/// correct**:
///
/// * the document being ingested when the panic happened was not added;
/// * every document added before it keeps exactly the counts it had;
/// * every document added *after* it counts exactly as it would have if the
///   panic had never happened.
///
/// The third point is the one worth stating, because it was once false. Counting
/// borrows reusable per-corpus buffers, and a panic raised *between two terms* —
/// which is where a caller's iterator raises one — unwound past their restore
/// and left them holding the abandoned document's state, which the next
/// document then mistook for its own: silently, with no wrong-looking number
/// anywhere until a score came out wrong. The buffers are now restored while
/// unwinding, so an interrupted ingestion cannot reach the next one.
///
/// ```
/// use verbora_tfidf::TfIdf;
///
/// let mut corpus = TfIdf::new();
/// corpus.add_terms(["a"]);
///
/// // An iterator that yields one term and then panics. (The default panic
/// // hook is muted first: the panic below is the point of the example, not a
/// // failure of it.)
/// # let previous = std::panic::take_hook();
/// # std::panic::set_hook(Box::new(|_| {}));
/// let unwound = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
///     corpus.add_terms((0..2).map(|i| if i == 0 { "a" } else { panic!("caller's iterator") }));
/// }));
/// # std::panic::set_hook(previous);
/// assert!(unwound.is_err());
///
/// // The abandoned document is not in the corpus, and the next one is right.
/// assert_eq!(corpus.len(), 1);
/// let d = corpus.add_terms(["b", "a", "a"]);
/// assert_eq!(corpus.term_count(d, "a"), Some(2));
/// assert_eq!(corpus.term_count(d, "b"), Some(1));
/// ```
///
/// # Examples
///
/// ```
/// use verbora_tfidf::TfIdf;
///
/// let mut corpus = TfIdf::new();
/// corpus.add_document("this document is about node");
/// corpus.add_document("this document is about ruby");
/// corpus.add_document("this document is about ruby and node");
/// corpus.add_document("this document is about node, it has node examples");
///
/// // `node` is in three of four documents.
/// assert_eq!(corpus.document_frequency("node"), 3);
/// assert_eq!(corpus.idf("node"), Some(1.0 + verbora_tfidf::natural_log(4.0 / 4.0)));
///
/// // …so its idf is exactly 1, and the scores are the raw counts.
/// assert_eq!(corpus.tfidfs("node"), [1.0, 0.0, 1.0, 2.0]);
///
/// // Ranked, best first.
/// let best = corpus.rank("node");
/// assert_eq!(best[0].document, 3);
/// assert_eq!(best[0].score, 2.0);
/// ```
#[derive(Debug, Clone)]
pub struct TfIdf {
    analyzer: Analyzer,
    interner: Interner,
    documents: Vec<Document>,
    /// Document frequency per term id. Maintained incrementally on every add
    /// and remove, which is what makes [`Self::idf`] `O(1)` rather than a scan
    /// of the corpus — and what removes the need for an idf cache, and with it
    /// every question about when a cache is stale.
    document_frequency: Vec<u32>,
    /// Reusable ingest buffers.
    scratch: Scratch,
}

impl Default for TfIdf {
    fn default() -> Self {
        Self::new()
    }
}

impl TfIdf {
    // -- construction -------------------------------------------------------

    /// An empty corpus with the default [`Analyzer`].
    #[must_use]
    pub fn new() -> Self {
        Self::with_analyzer(Analyzer::new())
    }

    /// An empty corpus with a chosen [`Analyzer`].
    ///
    /// The analyzer is fixed for the life of the corpus: changing how a term is
    /// derived halfway through would leave the documents added before the
    /// change keyed differently from the ones added after it, and no query
    /// could be right for both.
    #[must_use]
    pub fn with_analyzer(analyzer: Analyzer) -> Self {
        Self {
            analyzer,
            interner: Interner::default(),
            documents: Vec::new(),
            document_frequency: Vec::new(),
            scratch: Scratch::default(),
        }
    }

    /// The analyzer this corpus derives terms with.
    #[must_use]
    pub fn analyzer(&self) -> &Analyzer {
        &self.analyzer
    }

    // -- ingestion ----------------------------------------------------------

    /// Adds a document, returning its position in the corpus.
    ///
    /// `text` is run through [`Self::analyzer`]; see [`Analyzer`] for exactly
    /// what becomes a term. A document whose text yields no terms is still
    /// added, still occupies a position, and still counts towards `n`.
    ///
    /// # Panics
    ///
    /// Never on its own account below the limits in the type documentation —
    /// but the analyzer may be carrying a caller-supplied
    /// [`Tokenize`](crate::Tokenize), and a panic out of that propagates. The
    /// corpus is left without this document and otherwise correct, including
    /// for every document added afterwards; see "What a panic during ingestion
    /// leaves behind" on [`TfIdf`].
    pub fn add_document(&mut self, text: &str) -> usize {
        let document = build_from_text(
            &mut self.interner,
            &mut self.scratch,
            &self.analyzer,
            text,
            None,
        );
        self.push(document)
    }

    /// Adds a document under a caller-supplied key, returning its position.
    ///
    /// Keys are opaque: nothing is parsed out of them, and nothing requires
    /// them to be unique. [`Self::find_document`] returns the first match.
    pub fn add_document_with_key(&mut self, text: &str, key: impl Into<String>) -> usize {
        let key = key.into().into_boxed_str();
        let document = build_from_text(
            &mut self.interner,
            &mut self.scratch,
            &self.analyzer,
            text,
            Some(key),
        );
        self.push(document)
    }

    /// Adds a document from terms that are **already analyzed**, returning its
    /// position.
    ///
    /// The analyzer does not run: the strings are counted exactly as given,
    /// with no tokenizing, no folding and no stop-word filtering. That is the
    /// point — it is the entry point for terms produced by something this crate
    /// does not own, such as a stemmer or an n-gram builder.
    ///
    /// It is also the one way to put a term into the corpus that no query text
    /// can reach. `add_terms(["Node"])` on a lowercasing corpus stores `Node`,
    /// which `tfidf("Node", …)` will never find because the query is folded to
    /// `node`; use [`Self::tfidf_terms`] to query such a corpus.
    ///
    /// # Panics
    ///
    /// Never on its own account below the limits in the type documentation —
    /// but it drives `terms`, which is the caller's, and a panic out of that
    /// iterator propagates. The corpus is left without this document and
    /// otherwise correct, including for every document added afterwards; see
    /// "What a panic during ingestion leaves behind" on [`TfIdf`].
    pub fn add_terms<I, S>(&mut self, terms: I) -> usize
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let document = build_from_terms(&mut self.interner, &mut self.scratch, terms, None);
        self.push(document)
    }

    /// [`Self::add_terms`] under a caller-supplied key.
    pub fn add_terms_with_key<I, S>(&mut self, terms: I, key: impl Into<String>) -> usize
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let key = key.into().into_boxed_str();
        let document = build_from_terms(&mut self.interner, &mut self.scratch, terms, Some(key));
        self.push(document)
    }

    /// Adds several documents in order, returning the range of positions they
    /// were given.
    ///
    /// Exactly equivalent to calling [`Self::add_document`] once per element.
    /// It carries **no** performance advantage over that loop and is not
    /// intended to: it exists so that the sequential and parallel batch calls
    /// read the same, and so that a caller who wants the assigned positions
    /// does not have to collect them by hand.
    pub fn add_documents<S: AsRef<str>>(&mut self, texts: &[S]) -> Range<usize> {
        let start = self.documents.len();
        for text in texts {
            self.add_document(text.as_ref());
        }
        start..self.documents.len()
    }

    /// Adds several documents at once, analyzing them on Rayon's global thread
    /// pool, and returns the range of positions they were given.
    ///
    /// # Why this exists, and why it is shaped this way
    ///
    /// [`Self::add_document`] cannot simply be fanned out: it takes `&mut self`
    /// and every call mutates shared corpus state — the term table, the
    /// document-frequency table, the document list. Wrapping the corpus in a
    /// mutex would compile and would serialize exactly the work a parallel
    /// version exists to speed up.
    ///
    /// What *is* independent is the analyzer: tokenizing, folding and
    /// stop-word filtering are pure functions of the text and of the analyzer,
    /// and they are the dominant cost of ingesting a document. So this method
    /// splits ingestion in two:
    ///
    /// 1. **Parallel** — every text is run through
    ///    [`Analyzer::terms`](crate::Analyzer::terms), independently, on
    ///    however many cores Rayon offers.
    /// 2. **Sequential** — the resulting term lists are counted into the corpus
    ///    in the original order, through [`Self::add_terms`], which is the same
    ///    counting loop [`Self::add_document`] ends with.
    ///
    /// Because step 2 replays step 1's output in order through the unmodified
    /// sequential primitive, the result is *identical* to
    /// [`Self::add_documents`] on the same input — same positions, same counts,
    /// same term-id assignment order, same serialized bytes. `tests/parallel.rs`
    /// pins that.
    ///
    /// # What it costs
    ///
    /// One `Vec<Vec<String>>` sized to the batch, holding one `String` per term
    /// — the price of getting terms out of a parallel closure and into the
    /// sequential counter. [`Self::add_document`] allocates neither, because
    /// its terms are borrowed from the text it was given. That is the trade:
    /// one allocation per term, in exchange for running the analyzer on more
    /// than one core.
    ///
    /// # When to reach for it
    ///
    /// The crossover is **UNMEASURED** for this crate's current ingestion path.
    /// Fork-join has a fixed cost, so a handful of short documents will be
    /// slower this way; a large batch of substantial documents is what it is
    /// for. Measure your own corpus shape before adopting it.
    ///
    /// # Panics
    ///
    /// Never on its own account. A panic inside a custom
    /// [`Tokenize`](crate::Tokenize) propagates out of the offending Rayon
    /// worker under Rayon's own rules — and because that happens in the
    /// parallel phase, before anything is pushed, the corpus is left with
    /// **none** of the batch added.
    ///
    /// [`Self::add_documents`] differs only in how much of the batch survives:
    /// it pushes each document as it finishes, so a panic on the *k*th text
    /// leaves the first *k - 1* in the corpus and the rest out. Both leave a
    /// corpus that is correct for continued use — the guarantee is spelled out
    /// under "What a panic during ingestion leaves behind" on [`TfIdf`].
    ///
    /// # Examples
    ///
    /// ```
    /// # #[cfg(feature = "parallel")]
    /// # {
    /// use verbora_tfidf::TfIdf;
    ///
    /// let texts = ["this document is about node", "this document is about ruby"];
    ///
    /// let mut parallel = TfIdf::new();
    /// let added = parallel.par_add_documents(&texts);
    /// assert_eq!(added, 0..2);
    ///
    /// let mut sequential = TfIdf::new();
    /// sequential.add_documents(&texts);
    ///
    /// assert_eq!(parallel.to_json().unwrap(), sequential.to_json().unwrap());
    /// # }
    /// ```
    #[cfg(feature = "parallel")]
    #[cfg_attr(docsrs, doc(cfg(feature = "parallel")))]
    pub fn par_add_documents<S: AsRef<str> + Sync>(&mut self, texts: &[S]) -> Range<usize> {
        use rayon::prelude::*;

        let start = self.documents.len();
        if texts.is_empty() {
            return start..start;
        }
        let analyzer = &self.analyzer;
        let analyzed: Vec<Vec<String>> = texts
            .par_iter()
            .map(|text| analyzer.terms(text.as_ref()))
            .collect();
        for terms in analyzed {
            self.add_terms(terms);
        }
        start..self.documents.len()
    }

    /// Reads a UTF-8 file and adds its contents as a document, returning its
    /// position.
    ///
    /// A convenience over `std::fs::read_to_string` plus [`Self::add_document`]
    /// — no more. There is no encoding parameter: this crate indexes text, and
    /// decoding bytes into text is a separate concern with a separate contract.
    /// A file that is not valid UTF-8 is an error, not a stream of replacement
    /// characters silently indexed as terms.
    ///
    /// # Errors
    ///
    /// Whatever [`std::fs::read_to_string`] returns, including
    /// [`std::io::ErrorKind::InvalidData`] for a file that is not UTF-8.
    pub fn add_document_from_path(&mut self, path: impl AsRef<Path>) -> std::io::Result<usize> {
        let text = std::fs::read_to_string(path)?;
        Ok(self.add_document(&text))
    }

    /// Appends a built document and updates the derived tables.
    fn push(&mut self, document: Document) -> usize {
        // Every id in the document was issued by this corpus's term table, so
        // one resize to the table's size covers all of them.
        if self.document_frequency.len() < self.interner.len() {
            self.document_frequency.resize(self.interner.len(), 0);
        }
        for (id, count) in document.entries() {
            debug_assert!(
                *count > 0,
                "a stored entry always has at least one occurrence"
            );
            if let Some(frequency) = self.document_frequency.get_mut(id.index()) {
                *frequency += 1;
            }
        }
        let index = self.documents.len();
        self.documents.push(document);
        index
    }

    // -- the corpus ---------------------------------------------------------

    /// How many documents the corpus holds.
    #[must_use]
    pub fn len(&self) -> usize {
        self.documents.len()
    }

    /// Whether the corpus holds no documents at all.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.documents.is_empty()
    }

    /// Every document, in corpus order.
    #[must_use]
    pub fn documents(&self) -> &[Document] {
        &self.documents
    }

    /// The document at `index`, or `None` if there is none.
    #[must_use]
    pub fn document(&self, index: usize) -> Option<&Document> {
        self.documents.get(index)
    }

    /// The terms of a document with their counts, in first-occurrence order.
    ///
    /// Returns `None` when `index` is out of range. The order is the order the
    /// terms were first seen during ingestion, which is stable and is what a
    /// serialized corpus records; it is not sorted. For a ranked view, use
    /// [`Self::list_terms`].
    pub fn document_terms(&self, index: usize) -> Option<impl Iterator<Item = (&str, u32)>> {
        let document = self.documents.get(index)?;
        Some(
            document
                .entries()
                .iter()
                .map(|(id, count)| (self.interner.name_of(*id), *count)),
        )
    }

    /// The position of the first document whose key equals `key`.
    #[must_use]
    pub fn find_document(&self, key: &str) -> Option<usize> {
        self.documents
            .iter()
            .position(|d| d.key().is_some_and(|k| k == key))
    }

    /// Removes the document at `index` and returns it.
    ///
    /// Every later document **shifts down one position**, so an index held
    /// across a removal is no longer the document it named. Document
    /// frequencies are updated, so every idf and every score changes to match
    /// the smaller corpus.
    ///
    /// Returns `None`, changing nothing, when `index` is out of range.
    pub fn remove_document(&mut self, index: usize) -> Option<Document> {
        if index >= self.documents.len() {
            return None;
        }
        let removed = self.documents.remove(index);
        for (id, _) in removed.entries() {
            if let Some(count) = self.document_frequency.get_mut(id.index()) {
                *count = count.saturating_sub(1);
            }
        }
        Some(removed)
    }

    // -- statistics ---------------------------------------------------------

    /// How many terms occur in at least one document.
    ///
    /// Walks the whole term table, so it is `O(vocabulary)` rather than a
    /// stored counter. A term whose every document has been removed is not
    /// counted, even though the corpus still remembers its spelling.
    #[must_use]
    pub fn distinct_terms(&self) -> usize {
        self.document_frequency.iter().filter(|df| **df > 0).count()
    }

    /// How many documents contain `term` at least once.
    ///
    /// `term` is matched **literally** against the corpus's stored terms, which
    /// are analyzer output. Pass `analyzer().terms(text)` if what you have is
    /// raw text.
    #[must_use]
    pub fn document_frequency(&self, term: &str) -> u32 {
        self.interner
            .lookup(term)
            .map_or(0, |id| self.frequency_of(id))
    }

    /// How many times `term` occurs in the document at `index`.
    ///
    /// `Some(0)` means the document does not contain the term; `None` means
    /// there is no such document. The two are different answers and are never
    /// collapsed into one number.
    ///
    /// `term` is matched literally, as in [`Self::document_frequency`].
    #[must_use]
    pub fn term_count(&self, index: usize, term: &str) -> Option<u32> {
        let document = self.documents.get(index)?;
        Some(
            self.interner
                .lookup(term)
                .map_or(0, |id| document.count(id)),
        )
    }

    /// The inverse document frequency of `term`.
    ///
    /// `1 + ln(n / (1 + df))`, with `n` the number of documents and `df` the
    /// number of them containing `term`. The `1 +` inside the logarithm is what
    /// keeps an unseen term finite; the `1 +` outside keeps every idf strictly
    /// positive, so a term present in *every* document still contributes rather
    /// than vanishing.
    ///
    /// Returns `None` for an **empty corpus**: `ln(0 / 1)` is `-∞`, and an
    /// infinity escaping into a caller's arithmetic is as damaging as a `NaN`.
    /// There is no inverse document frequency when there are no documents, and
    /// that is what `None` says.
    ///
    /// `term` is matched literally, as in [`Self::document_frequency`].
    ///
    /// # Examples
    ///
    /// ```
    /// use verbora_tfidf::{TfIdf, natural_log};
    ///
    /// let mut corpus = TfIdf::new();
    /// assert_eq!(corpus.idf("anything"), None);
    ///
    /// corpus.add_document("node");
    /// corpus.add_document("ruby");
    /// // Present in one of two documents: 1 + ln(2/2) = 1.
    /// assert_eq!(corpus.idf("node"), Some(1.0));
    /// // Present in neither: 1 + ln(2/1).
    /// assert_eq!(corpus.idf("perl"), Some(1.0 + natural_log(2.0)));
    /// ```
    #[must_use]
    pub fn idf(&self, term: &str) -> Option<f64> {
        if self.documents.is_empty() {
            return None;
        }
        Some(idf_value(
            self.documents.len(),
            self.document_frequency(term),
        ))
    }

    /// The document frequency stored for an id.
    fn frequency_of(&self, id: TermId) -> u32 {
        self.document_frequency
            .get(id.index())
            .copied()
            .unwrap_or(0)
    }

    // -- queries ------------------------------------------------------------

    /// Resolves a query's terms to `(id, idf)` pairs, once, in query order.
    ///
    /// Empty for an empty corpus, where no idf is defined.
    fn resolve<I, S>(&self, terms: I) -> Vec<(Option<TermId>, f64)>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        if self.documents.is_empty() {
            return Vec::new();
        }
        terms
            .into_iter()
            .map(|term| self.weigh(term.as_ref()))
            .collect()
    }

    /// [`Self::resolve`] for a query given as text, which the analyzer runs on.
    fn resolve_text(&self, query: &str) -> Vec<(Option<TermId>, f64)> {
        if self.documents.is_empty() {
            return Vec::new();
        }
        let mut resolved = Vec::new();
        let mut scratch = Vec::new();
        self.analyzer.for_each_term(query, &mut scratch, |term| {
            resolved.push(self.weigh(term));
        });
        resolved
    }

    /// One query term's id and idf.
    fn weigh(&self, term: &str) -> (Option<TermId>, f64) {
        let id = self.interner.lookup(term);
        let df = id.map_or(0, |id| self.frequency_of(id));
        (id, idf_value(self.documents.len(), df))
    }

    /// `Σ count(t, d) · idf(t)`, left to right over `resolved`.
    fn score(resolved: &[(Option<TermId>, f64)], document: &Document) -> f64 {
        let mut sum = 0.0;
        for (id, idf) in resolved {
            let count = id.map_or(0, |id| document.count(id));
            sum += f64::from(count) * idf;
        }
        sum
    }

    /// The TF-IDF score of `query` against the document at `index`.
    ///
    /// `query` is analyzed exactly as a document would be, so a query term that
    /// this corpus's analyzer would have removed from a document is removed
    /// from the query too.
    ///
    /// Returns `None` when `index` is out of range — which includes every index
    /// on an empty corpus. A score is never used to signal "no such document".
    ///
    /// # Examples
    ///
    /// ```
    /// use verbora_tfidf::{TfIdf, natural_log};
    ///
    /// let mut corpus = TfIdf::new();
    /// corpus.add_document("node and ruby");
    /// corpus.add_document("node and node");
    ///
    /// // `node` is in both documents, so idf = 1 + ln(2 / (1 + 2)), and it
    /// // occurs twice in document 1.
    /// let idf = 1.0 + natural_log(2.0 / 3.0);
    /// assert_eq!(corpus.tfidf("node", 1), Some(2.0 * idf));
    /// assert_eq!(corpus.tfidf("", 0), Some(0.0));
    /// assert_eq!(corpus.tfidf("node", 9), None);
    /// ```
    #[must_use]
    pub fn tfidf(&self, query: &str, index: usize) -> Option<f64> {
        let document = self.documents.get(index)?;
        Some(Self::score(&self.resolve_text(query), document))
    }

    /// [`Self::tfidf`] for a query whose terms are **already analyzed**.
    ///
    /// The analyzer does not run: the strings are looked up exactly as given.
    /// Use this to query a corpus built with [`Self::add_terms`], or when the
    /// query terms came out of the same pipeline that produced the documents.
    #[must_use]
    pub fn tfidf_terms<I, S>(&self, terms: I, index: usize) -> Option<f64>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let resolved = self.resolve(terms);
        let document = self.documents.get(index)?;
        Some(Self::score(&resolved, document))
    }

    /// [`Self::tfidf`] against every document, in corpus order.
    ///
    /// The query is analyzed and weighted **once** and then applied to each
    /// document, which is why this is the right call when you want more than
    /// one document's score. Empty for an empty corpus.
    #[must_use]
    pub fn tfidfs(&self, query: &str) -> Vec<f64> {
        let resolved = self.resolve_text(query);
        self.documents
            .iter()
            .map(|d| Self::score(&resolved, d))
            .collect()
    }

    /// [`Self::tfidfs`] for a query whose terms are already analyzed.
    #[must_use]
    pub fn tfidfs_terms<I, S>(&self, terms: I) -> Vec<f64>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let resolved = self.resolve(terms);
        self.documents
            .iter()
            .map(|d| Self::score(&resolved, d))
            .collect()
    }

    /// Every document scored against `query`, best first.
    ///
    /// The order is a **total** order, so it is the same on every run: by score
    /// descending, and between equal scores by corpus position ascending.
    /// Documents scoring `0.0` are included — the caller decides what a
    /// threshold means.
    #[must_use]
    pub fn rank(&self, query: &str) -> Vec<DocumentScore> {
        let resolved = self.resolve_text(query);
        let mut ranked: Vec<DocumentScore> = self
            .documents
            .iter()
            .enumerate()
            .map(|(document, d)| DocumentScore {
                document,
                score: Self::score(&resolved, d),
            })
            .collect();
        ranked.sort_by(|a, b| {
            b.score
                .total_cmp(&a.score)
                .then_with(|| a.document.cmp(&b.document))
        });
        ranked
    }

    /// Every term of one document, scored and ranked.
    ///
    /// The order is a **total** order, so it is the same on every run: by
    /// `tfidf` descending, and between equal scores by term ascending in
    /// Unicode scalar order. Returns `None` when `index` is out of range.
    ///
    /// # Examples
    ///
    /// ```
    /// use verbora_tfidf::TfIdf;
    ///
    /// let mut corpus = TfIdf::new();
    /// corpus.add_document("node node ruby");
    /// corpus.add_document("ruby");
    ///
    /// let terms = corpus.list_terms(0).unwrap();
    /// assert_eq!(terms[0].term, "node");
    /// assert_eq!(terms[0].count, 2);
    /// assert!(terms[0].tfidf > terms[1].tfidf);
    /// ```
    #[must_use]
    pub fn list_terms(&self, index: usize) -> Option<Vec<TermScore>> {
        let document = self.documents.get(index)?;
        let n = self.documents.len();
        let mut scored: Vec<TermScore> = document
            .entries()
            .iter()
            .map(|(id, count)| {
                let idf = idf_value(n, self.frequency_of(*id));
                TermScore {
                    term: self.interner.name_of(*id).to_owned(),
                    count: *count,
                    idf,
                    tfidf: f64::from(*count) * idf,
                }
            })
            .collect();
        scored.sort_by(|a, b| {
            b.tfidf
                .total_cmp(&a.tfidf)
                .then_with(|| a.term.cmp(&b.term))
        });
        Some(scored)
    }

    // -- persistence --------------------------------------------------------

    /// Serializes the corpus, stamped with the build that produced its terms.
    ///
    /// # The shape
    ///
    /// ```json
    /// {
    ///   "_verbora": {"schema": 3, "unicode": "17.0.0", "lowercase": "0123456789abcdef"},
    ///   "analyzer": {"case_fold": "lowercase", "stop_words": ["a", "the"]},
    ///   "documents": [
    ///     {"key": "doc-1", "terms": [["node", 2], ["ruby", 1]]},
    ///     {"terms": []}
    ///   ]
    /// }
    /// ```
    ///
    /// Three deliberate choices:
    ///
    /// * **Terms are an array of pairs, not an object.** An object would make
    ///   term order a property of the JSON parser and would leave duplicate keys
    ///   with no defined meaning. An array preserves first-occurrence order
    ///   exactly and makes a duplicate detectable, which
    ///   [`RestoreError::DuplicateTerm`] reports rather than silently resolving.
    /// * **The analyzer travels with the corpus.** The stop-word list decides
    ///   which terms exist at all and the case fold decides how they are
    ///   spelled, so an artifact that did not carry them could not be queried
    ///   consistently after a restore. [`Self::from_json`] installs the
    ///   *recorded* analyzer, not the default one.
    /// * **`key` and `stop_words` are omitted when absent**, rather than written
    ///   as `null`. Absent and `null` both read back as absent.
    ///
    /// The `_verbora` member is the compatibility stamp; see
    /// [`ArtifactStamp`](crate::ArtifactStamp)
    /// for what it covers and why loading without a matching one is refused.
    ///
    /// # Errors
    ///
    /// [`ExportError::CustomTokenizer`] when the analyzer runs a tokenizer
    /// installed through
    /// [`Analyzer::with_tokenizer`](crate::Analyzer::with_tokenizer): no
    /// artifact can describe how those terms were derived, so this refuses to
    /// write one that would look validated and would not be.
    pub fn to_json(&self) -> Result<String, ExportError> {
        if !self.analyzer.uses_default_tokenizer() {
            return Err(ExportError::CustomTokenizer);
        }
        let artifact = Artifact {
            stamp: crate::stamp::ArtifactStamp::current().to_json(),
            analyzer: AnalyzerRecord {
                case_fold: self.analyzer.case_fold().as_str().to_owned(),
                stop_words: self.analyzer.stop_words().map(|list| list.words().to_vec()),
            },
            documents: self
                .documents
                .iter()
                .map(|d| DocumentRecord {
                    key: d.key().map(str::to_owned),
                    terms: d
                        .entries()
                        .iter()
                        .map(|(id, count)| (self.interner.name_of(*id).to_owned(), *count))
                        .collect(),
                })
                .collect(),
        };
        serde_json::to_string(&artifact).map_err(ExportError::Json)
    }

    /// Restores a corpus written by [`Self::to_json`].
    ///
    /// The compatibility stamp is checked **before** any of the body is
    /// interpreted, so an artifact from another build is refused rather than
    /// half-read.
    ///
    /// # Errors
    ///
    /// [`RestoreError`], which separates a damaged file from a stale one from
    /// one whose contents a corpus cannot mean; see that type.
    ///
    /// # Examples
    ///
    /// ```
    /// use verbora_tfidf::TfIdf;
    ///
    /// let mut corpus = TfIdf::new();
    /// corpus.add_document_with_key("node and ruby", "doc-1");
    ///
    /// let json = corpus.to_json().unwrap();
    /// let restored = TfIdf::from_json(&json).unwrap();
    ///
    /// assert_eq!(restored.find_document("doc-1"), Some(0));
    /// assert_eq!(restored.tfidfs("node"), corpus.tfidfs("node"));
    /// ```
    pub fn from_json(json: &str) -> Result<Self, RestoreError> {
        let value: serde_json::Value = serde_json::from_str(json).map_err(RestoreError::Parse)?;
        crate::stamp::verify(&value)?;
        let artifact: Artifact = serde_json::from_value(value).map_err(RestoreError::Parse)?;

        let case_fold = CaseFold::parse(&artifact.analyzer.case_fold)
            .ok_or_else(|| RestoreError::UnknownCaseFold(artifact.analyzer.case_fold.clone()))?;
        let mut analyzer = Analyzer::new().with_case_fold(case_fold);
        if let Some(words) = artifact.analyzer.stop_words {
            analyzer = analyzer.with_stop_words(verbora_core::StopWords::from_iter_of(words));
        }

        let mut corpus = Self::with_analyzer(analyzer);
        let mut seen: FxHashSet<TermId> = FxHashSet::default();
        for (index, record) in artifact.documents.into_iter().enumerate() {
            seen.clear();
            let mut entries = Vec::with_capacity(record.terms.len());
            let mut total: u64 = 0;
            for (term, count) in record.terms {
                if count == 0 {
                    return Err(RestoreError::ZeroCount {
                        document: index,
                        term,
                    });
                }
                let id = corpus.interner.intern(&term);
                if !seen.insert(id) {
                    return Err(RestoreError::DuplicateTerm {
                        document: index,
                        term,
                    });
                }
                total = total.saturating_add(u64::from(count));
                entries.push((id, count));
            }
            corpus.push(Document::from_parts(
                record.key.map(String::into_boxed_str),
                entries,
                total,
            ));
        }
        Ok(corpus)
    }
}

/// `1 + ln(n / (1 + df))`.
///
/// `documents` must be at least one; every caller checks, and
/// [`TfIdf::idf`] is the checked public form.
fn idf_value(documents: usize, document_frequency: u32) -> f64 {
    debug_assert!(documents >= 1, "an empty corpus has no idf");
    #[expect(
        clippy::cast_precision_loss,
        reason = "a corpus of 2^53 documents does not fit in memory"
    )]
    let n = documents as f64;
    1.0 + natural_log(n / (1.0 + f64::from(document_frequency)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use verbora_core::StopWords;

    fn corpus(texts: &[&str]) -> TfIdf {
        let mut c = TfIdf::new();
        c.add_documents(texts);
        c
    }

    fn terms_of(c: &TfIdf, index: usize) -> Vec<(String, u32)> {
        c.document_terms(index)
            .expect("document in range")
            .map(|(term, count)| (term.to_owned(), count))
            .collect()
    }

    fn names_of(c: &TfIdf, index: usize) -> Vec<String> {
        terms_of(c, index).into_iter().map(|(t, _)| t).collect()
    }

    // -- the model ----------------------------------------------------------

    #[test]
    fn the_worked_example_from_the_type_documentation() {
        let c = corpus(&[
            "this document is about node",
            "this document is about ruby",
            "this document is about ruby and node",
            "this document is about node, it has node examples",
        ]);
        assert_eq!(c.len(), 4);
        assert_eq!(c.document_frequency("node"), 3);
        // 1 + ln(4 / (1 + 3)) = 1 + ln 1 = 1.
        assert_eq!(c.idf("node"), Some(1.0));
        assert_eq!(c.tfidfs("node"), [1.0, 0.0, 1.0, 2.0]);
        assert_eq!(c.tfidf("ruby", 3), Some(0.0));
    }

    #[test]
    fn idf_is_the_stated_formula_for_every_reachable_document_frequency() {
        // Enumerated: a corpus of eight documents, and a term placed in
        // 0..=8 of them. Every expectation is the formula, evaluated.
        for present in 0..=8usize {
            let mut c = TfIdf::new();
            for i in 0..8usize {
                if i < present {
                    c.add_document("target filler");
                } else {
                    c.add_document("filler");
                }
            }
            let df = u32::try_from(present).expect("small");
            assert_eq!(c.document_frequency("target"), df);
            let want = 1.0 + natural_log(8.0 / (1.0 + f64::from(df)));
            assert_eq!(c.idf("target"), Some(want), "df = {df}");
            assert!(want > 0.0, "idf must stay strictly positive at df = {df}");
            assert!(want.is_finite());
        }
    }

    #[test]
    fn idf_is_non_increasing_in_document_frequency() {
        let mut previous = f64::INFINITY;
        for present in 0..=16usize {
            let mut c = TfIdf::new();
            for i in 0..16usize {
                c.add_document(if i < present {
                    "target filler"
                } else {
                    "filler"
                });
            }
            let idf = c.idf("target").expect("non-empty corpus");
            assert!(idf <= previous, "idf rose at df = {present}");
            previous = idf;
        }
    }

    #[test]
    fn a_term_in_no_document_is_finite_and_contributes_nothing() {
        let c = corpus(&["node", "ruby"]);
        assert_eq!(c.document_frequency("perl"), 0);
        assert_eq!(c.idf("perl"), Some(1.0 + natural_log(2.0)));
        assert_eq!(c.term_count(0, "perl"), Some(0));
        assert_eq!(c.tfidf("perl", 0), Some(0.0));
        assert_eq!(c.tfidfs("perl"), [0.0, 0.0]);
    }

    #[test]
    fn an_empty_corpus_has_no_idf_and_no_scores() {
        let c = TfIdf::new();
        assert!(c.is_empty());
        assert_eq!(c.len(), 0);
        assert_eq!(c.idf("anything"), None);
        assert_eq!(c.document_frequency("anything"), 0);
        assert_eq!(c.term_count(0, "anything"), None);
        assert_eq!(c.tfidf("anything", 0), None);
        assert_eq!(c.tfidf_terms(["anything"], 0), None);
        assert!(c.tfidfs("anything").is_empty());
        assert!(c.rank("anything").is_empty());
        assert_eq!(c.list_terms(0), None);
        assert!(c.document(0).is_none());
        assert_eq!(c.distinct_terms(), 0);
    }

    #[test]
    fn a_document_with_no_terms_still_counts_towards_n() {
        let c = corpus(&["node", "   ,,,   ", ""]);
        assert_eq!(c.len(), 3);
        assert!(c.document(1).expect("in range").is_empty());
        assert!(c.document(2).expect("in range").is_empty());
        // n = 3, df = 1.
        assert_eq!(c.idf("node"), Some(1.0 + natural_log(3.0 / 2.0)));
        assert_eq!(c.tfidf("node", 1), Some(0.0));
        assert_eq!(c.list_terms(1), Some(Vec::new()));
    }

    #[test]
    fn an_out_of_range_index_is_none_and_never_a_score() {
        let c = corpus(&["node"]);
        assert_eq!(c.tfidf("node", 1), None);
        assert_eq!(c.tfidf_terms(["node"], 1), None);
        assert_eq!(c.term_count(1, "node"), None);
        assert_eq!(c.list_terms(1), None);
        assert!(c.document_terms(1).is_none());
        assert_eq!(c.tfidf("node", usize::MAX), None);
    }

    #[test]
    fn an_empty_query_scores_exactly_zero() {
        let c = corpus(&["node ruby"]);
        assert_eq!(c.tfidf("", 0), Some(0.0));
        assert_eq!(c.tfidf("   ,,, ", 0), Some(0.0));
        assert_eq!(c.tfidf_terms(Vec::<String>::new(), 0), Some(0.0));
        assert_eq!(c.tfidfs(""), [0.0]);
    }

    #[test]
    fn a_repeated_query_term_counts_once_per_occurrence() {
        let c = corpus(&["node", "ruby"]);
        let once = c.tfidf("node", 0).expect("in range");
        assert_eq!(c.tfidf("node node", 0), Some(once + once));
        assert_eq!(c.tfidf("node node node", 0), Some(once + once + once));
    }

    /// No score may be `NaN` or infinite — enumerated over every document, an
    /// adversarial query set, and corpus sizes spanning the interesting range.
    #[test]
    fn no_score_is_ever_nan_or_infinite() {
        let vocabulary = [
            "node",
            "ruby",
            "",
            "   ",
            "日本語",
            "😀",
            "don't",
            "3.14",
            "İstanbul",
            "ΑΣ",
            "a-very-long-term-well-past-eight-bytes",
        ];
        for n in [1usize, 2, 3, 8, 33] {
            let mut c = TfIdf::new();
            for i in 0..n {
                c.add_document(vocabulary[i % vocabulary.len()]);
            }
            for query in vocabulary {
                for d in 0..n {
                    let score = c.tfidf(query, d).expect("in range");
                    assert!(score.is_finite(), "tfidf({query:?}, {d}) = {score:?}");
                }
                for score in c.tfidfs(query) {
                    assert!(score.is_finite(), "tfidfs({query:?}) held {score:?}");
                }
                for ranked in c.rank(query) {
                    assert!(ranked.score.is_finite());
                }
                if let Some(idf) = c.idf(query) {
                    assert!(idf.is_finite() && idf > 0.0, "idf({query:?}) = {idf:?}");
                }
            }
            for d in 0..n {
                for scored in c.list_terms(d).expect("in range") {
                    assert!(scored.idf.is_finite());
                    assert!(scored.tfidf.is_finite());
                }
            }
        }
    }

    #[test]
    fn accumulation_is_left_to_right_over_the_query() {
        // Chosen so the two groupings differ in the last bit: the running sum
        // must be `((0 + a) + b) + c`, not any re-association of it.
        let mut c = TfIdf::new();
        c.add_terms(std::iter::repeat_n("a", 1));
        c.add_terms(std::iter::repeat_n("b", 3));
        c.add_terms(std::iter::repeat_n("c", 7));
        c.add_terms(["a", "b", "c"]);

        let idf = |t: &str| c.idf(t).expect("non-empty corpus");
        let d = 3;
        let want = {
            let mut sum = 0.0f64;
            sum += 1.0 * idf("a");
            sum += 1.0 * idf("b");
            sum += 1.0 * idf("c");
            sum
        };
        assert_eq!(
            c.tfidf_terms(["a", "b", "c"], d)
                .expect("in range")
                .to_bits(),
            want.to_bits()
        );
    }

    // -- ingestion ----------------------------------------------------------

    #[test]
    fn add_document_returns_the_position_it_used() {
        let mut c = TfIdf::new();
        assert_eq!(c.add_document("one"), 0);
        assert_eq!(c.add_document_with_key("two", "k"), 1);
        assert_eq!(c.add_terms(["three"]), 2);
        assert_eq!(c.add_terms_with_key(["four"], "k4"), 3);
        assert_eq!(c.add_documents(&["five", "six"]), 4..6);
        assert_eq!(c.len(), 6);
        assert_eq!(c.document(1).and_then(Document::key), Some("k"));
        assert_eq!(c.document(3).and_then(Document::key), Some("k4"));
        assert_eq!(c.document(0).and_then(Document::key), None);
    }

    #[test]
    fn keys_are_opaque_and_need_not_be_unique() {
        let mut c = TfIdf::new();
        c.add_document_with_key("a", "same");
        c.add_document_with_key("b", "same");
        c.add_document_with_key("c", "");
        assert_eq!(c.find_document("same"), Some(0));
        assert_eq!(c.find_document(""), Some(2));
        assert_eq!(c.find_document("absent"), None);
    }

    #[test]
    fn add_terms_bypasses_the_whole_analyzer() {
        let mut c =
            TfIdf::with_analyzer(Analyzer::new().with_stop_words(StopWords::from_iter_of(["the"])));
        c.add_terms(["The", "the", "THE", "don't split"]);
        assert_eq!(names_of(&c, 0), ["The", "the", "THE", "don't split"]);
        // …and such a term is unreachable from a text query.
        assert_eq!(c.tfidf("The", 0), Some(0.0));
        // One document holding it, out of one: 1 + ln(1/2).
        let single = 1.0 + natural_log(0.5);
        assert_eq!(c.tfidf_terms(["The"], 0), Some(single));
    }

    #[test]
    fn documents_and_queries_go_through_the_same_analyzer() {
        let mut c =
            TfIdf::with_analyzer(Analyzer::new().with_stop_words(StopWords::from_iter_of(["the"])));
        c.add_document("The quick brown fox");
        assert_eq!(names_of(&c, 0), ["quick", "brown", "fox"]);
        // The query's stop word is dropped too, so this is the empty query.
        assert_eq!(c.tfidf("the", 0), Some(0.0));
        // Folding applies to the query as well.
        assert_eq!(c.tfidf("QUICK", 0), c.tfidf("quick", 0));
    }

    #[test]
    fn case_folding_is_the_analyzers_choice() {
        let folded = corpus(&["Node NODE node"]);
        assert_eq!(terms_of(&folded, 0), [("node".to_owned(), 3)]);

        let mut exact = TfIdf::with_analyzer(Analyzer::new().with_case_fold(CaseFold::None));
        exact.add_document("Node NODE node");
        assert_eq!(names_of(&exact, 0), ["Node", "NODE", "node"]);
        // One document, and each spelling occurs in it: 1 + ln(1/2).
        assert_eq!(exact.tfidf("Node", 0), Some(1.0 + natural_log(0.5)));
    }

    #[test]
    fn segmentation_is_the_tokenizers_and_reaches_ingestion() {
        // The four ASCII shapes a hand-rolled letters-and-digits scan splits.
        let c = corpus(&["don't 3.14 1,000 a:b node_js"]);
        assert_eq!(
            names_of(&c, 0),
            ["don't", "3.14", "1,000", "a:b", "node_js"]
        );
        // …and each is reachable as a query, which is the point.
        // One document, one occurrence each: 1 + ln(1/2).
        let single = 1.0 + natural_log(0.5);
        for term in ["don't", "3.14", "1,000", "a:b", "node_js"] {
            assert_eq!(c.term_count(0, term), Some(1), "{term}");
            assert_eq!(c.tfidf(term, 0), Some(single), "{term}");
        }
    }

    #[test]
    fn non_ascii_input_classes_survive_ingestion() {
        let c = corpus(&[
            "naïve café crème brûlée",
            "Москва и Ленинград",
            "İstanbul",
            "日本語 test",
            "😀abc😀",
            "𝕳𝖊𝖑𝖑𝖔",
        ]);
        assert_eq!(names_of(&c, 0), ["naïve", "café", "crème", "brûlée"]);
        assert_eq!(names_of(&c, 1), ["москва", "и", "ленинград"]);
        assert_eq!(names_of(&c, 2), ["i\u{307}stanbul"]);
        assert_eq!(names_of(&c, 3), ["日", "本", "語", "test"]);
        assert_eq!(names_of(&c, 4), ["abc"]);
        assert_eq!(names_of(&c, 5), ["𝕳𝖊𝖑𝖑𝖔".to_lowercase()]);
    }

    #[test]
    fn a_single_character_document_is_a_single_term() {
        let c = corpus(&["a", "1", "é"]);
        for (index, term) in ["a", "1", "é"].iter().enumerate() {
            assert_eq!(names_of(&c, index), [*term]);
            assert_eq!(c.term_count(index, term), Some(1));
        }
    }

    #[test]
    fn a_long_document_counts_every_occurrence() {
        let text = "lorem ipsum ".repeat(20_000);
        let mut c = TfIdf::new();
        c.add_document(&text);
        assert_eq!(names_of(&c, 0), ["lorem", "ipsum"]);
        assert_eq!(c.term_count(0, "lorem"), Some(20_000));
        assert_eq!(c.document(0).expect("in range").total_terms(), 40_000);

        let long_word = "a".repeat(100_000);
        c.add_document(&long_word);
        assert_eq!(names_of(&c, 1), [long_word]);
    }

    #[test]
    fn a_custom_tokenizer_reaches_ingestion_and_queries() {
        #[derive(Debug)]
        struct SplitOnApostrophe;
        impl crate::Tokenize for SplitOnApostrophe {
            fn tokenize_into(&self, text: &str, out: &mut Vec<String>) {
                out.extend(
                    text.split(|c: char| c.is_whitespace() || c == '\'')
                        .filter(|piece| !piece.is_empty())
                        .map(str::to_owned),
                );
            }
        }

        let mut c = TfIdf::with_analyzer(
            Analyzer::new().with_tokenizer(std::sync::Arc::new(SplitOnApostrophe)),
        );
        c.add_document("this isn't node");
        assert_eq!(names_of(&c, 0), ["this", "isn", "t", "node"]);
        assert_eq!(c.tfidf("isn't", 0), Some(2.0 * c.idf("isn").expect("n>0")));
    }

    // -- removal ------------------------------------------------------------

    #[test]
    fn removing_a_document_updates_every_derived_number() {
        let mut c = corpus(&["shared term", "shared other"]);
        assert_eq!(c.idf("shared"), Some(1.0 + natural_log(2.0 / 3.0)));
        let removed = c.remove_document(0).expect("in range");
        assert_eq!(removed.distinct_terms(), 2);
        assert_eq!(c.len(), 1);
        assert_eq!(c.document_frequency("shared"), 1);
        assert_eq!(c.document_frequency("term"), 0);
        assert_eq!(c.idf("shared"), Some(1.0 + natural_log(1.0 / 2.0)));
        // The removed document's own terms are still spelled in the table but
        // occur nowhere.
        assert_eq!(c.term_count(0, "term"), Some(0));
        assert_eq!(c.distinct_terms(), 2);
    }

    #[test]
    fn removing_shifts_later_documents_down() {
        let mut c = corpus(&["zero", "one", "two"]);
        assert_eq!(c.remove_document(0).map(|d| d.distinct_terms()), Some(1));
        assert_eq!(names_of(&c, 0), ["one"]);
        assert_eq!(names_of(&c, 1), ["two"]);
        assert!(c.remove_document(5).is_none());
        assert_eq!(c.len(), 2);
    }

    #[test]
    fn removing_every_document_returns_to_the_empty_corpus() {
        let mut c = corpus(&["a", "b"]);
        assert!(c.remove_document(0).is_some());
        assert!(c.remove_document(0).is_some());
        assert!(c.is_empty());
        assert_eq!(c.idf("a"), None);
        assert_eq!(c.document_frequency("a"), 0);
        assert_eq!(c.distinct_terms(), 0);
    }

    // -- ordering -----------------------------------------------------------

    #[test]
    fn rank_is_a_total_order_and_stable_across_runs() {
        let c = corpus(&["node", "node node", "ruby", "node node node", "node"]);
        let ranked = c.rank("node");
        let order: Vec<usize> = ranked.iter().map(|r| r.document).collect();
        // Scores 1, 2, 0, 3, 1 -> descending, ties by position ascending.
        assert_eq!(order, [3, 1, 0, 4, 2]);
        // Running it again gives the same permutation.
        assert_eq!(c.rank("node"), ranked);
    }

    #[test]
    fn list_terms_breaks_ties_by_term_ascending() {
        // Every term occurs once in one document, so every score is equal and
        // the tiebreak is the whole order.
        let c = corpus(&["zebra apple mango"]);
        let terms: Vec<String> = c
            .list_terms(0)
            .expect("in range")
            .into_iter()
            .map(|t| t.term)
            .collect();
        assert_eq!(terms, ["apple", "mango", "zebra"]);
        // Ingestion order is a different, also-stable order.
        assert_eq!(names_of(&c, 0), ["zebra", "apple", "mango"]);
    }

    #[test]
    fn list_terms_scores_match_the_single_term_query() {
        let c = corpus(&["node node ruby perl", "ruby", "perl perl"]);
        for scored in c.list_terms(0).expect("in range") {
            assert_eq!(
                scored.idf,
                c.idf(&scored.term).expect("n>0"),
                "{}",
                scored.term
            );
            assert_eq!(
                scored.count,
                c.term_count(0, &scored.term).expect("in range")
            );
            assert_eq!(
                scored.tfidf,
                c.tfidf_terms([&scored.term], 0).expect("in range"),
                "{}",
                scored.term
            );
        }
    }

    // -- API pairs agree ----------------------------------------------------

    #[test]
    fn the_text_and_term_query_forms_agree_when_the_terms_match() {
        let c = corpus(&[
            "this document is about node",
            "this document is about ruby",
            "node_js and n0de and über and 3.14",
        ]);
        for query in [
            "node",
            "the quick brown fox",
            "Node.js",
            "node, ruby and rails",
            "isn't",
            "über",
            "ÜBER",
            "İstanbul",
            "日本語",
            "😀",
            "",
            "   ",
            "3.14",
        ] {
            let analyzed = c.analyzer().terms(query);
            for d in 0..c.len() {
                assert_eq!(
                    c.tfidf(query, d).map(f64::to_bits),
                    c.tfidf_terms(&analyzed, d).map(f64::to_bits),
                    "tfidf({query:?}, {d})"
                );
            }
            assert_eq!(c.tfidfs(query), c.tfidfs_terms(&analyzed), "{query:?}");
            let ranked: Vec<f64> = {
                let mut scores = vec![0.0; c.len()];
                for r in c.rank(query) {
                    scores[r.document] = r.score;
                }
                scores
            };
            assert_eq!(ranked, c.tfidfs(query), "rank({query:?})");
        }
    }

    #[test]
    fn add_documents_matches_a_loop_of_add_document() {
        let texts = ["node and ruby", "", "İstanbul don't 3.14", "node node"];
        let mut batched = TfIdf::new();
        batched.add_documents(&texts);
        let mut looped = TfIdf::new();
        for text in texts {
            looped.add_document(text);
        }
        assert_eq!(
            batched.to_json().expect("default tokenizer"),
            looped.to_json().expect("default tokenizer")
        );
    }

    // -- persistence --------------------------------------------------------

    #[test]
    fn a_corpus_round_trips_through_json() {
        let mut c = TfIdf::with_analyzer(
            Analyzer::new().with_stop_words(StopWords::from_iter_of(["is", "about"])),
        );
        c.add_document_with_key("this document is about node", "un");
        c.add_document_with_key("this document is about ruby", "deux");
        c.add_document("");

        let json = c.to_json().expect("default tokenizer");
        let back = TfIdf::from_json(&json).expect("its own output");

        assert_eq!(back.len(), c.len());
        assert_eq!(back.find_document("deux"), Some(1));
        assert_eq!(back.analyzer().case_fold(), CaseFold::Lowercase);
        assert_eq!(
            back.analyzer().stop_words().map(StopWords::words),
            Some(["is".to_owned(), "about".to_owned()].as_slice())
        );
        for d in 0..c.len() {
            assert_eq!(terms_of(&back, d), terms_of(&c, d), "document {d}");
        }
        assert_eq!(back.tfidfs("node"), c.tfidfs("node"));
        // The restored analyzer still filters, so a stop word is still dropped.
        assert_eq!(back.tfidf("about", 0), Some(0.0));
        assert_eq!(back.to_json().expect("default tokenizer"), json);
    }

    #[test]
    fn the_recorded_case_fold_survives_a_round_trip() {
        let mut c = TfIdf::with_analyzer(Analyzer::new().with_case_fold(CaseFold::None));
        c.add_document("Node NODE");
        let back = TfIdf::from_json(&c.to_json().expect("default tokenizer")).expect("round-trips");
        assert_eq!(back.analyzer().case_fold(), CaseFold::None);
        assert_eq!(back.tfidf("Node", 0), Some(1.0 + natural_log(0.5)));
        assert_eq!(back.tfidf("node", 0), Some(0.0));
    }

    #[test]
    fn a_custom_tokenizer_makes_serialization_impossible() {
        #[derive(Debug)]
        struct Whitespace;
        impl crate::Tokenize for Whitespace {
            fn tokenize_into(&self, text: &str, out: &mut Vec<String>) {
                out.extend(text.split_whitespace().map(str::to_owned));
            }
        }
        let mut c =
            TfIdf::with_analyzer(Analyzer::new().with_tokenizer(std::sync::Arc::new(Whitespace)));
        c.add_document("node ruby");
        assert!(matches!(
            c.to_json(),
            Err(crate::ExportError::CustomTokenizer)
        ));
    }

    #[test]
    fn the_serialized_shape_is_the_documented_one() {
        let mut c = TfIdf::new();
        c.add_document_with_key("node ruby node", "k");
        let json = c.to_json().expect("default tokenizer");
        let value: serde_json::Value = serde_json::from_str(&json).expect("valid JSON");
        assert_eq!(value["analyzer"]["case_fold"], "lowercase");
        assert!(value["analyzer"].get("stop_words").is_none());
        assert_eq!(value["documents"][0]["key"], "k");
        assert_eq!(
            value["documents"][0]["terms"],
            serde_json::json!([["node", 2], ["ruby", 1]])
        );
        assert!(value.get(crate::stamp::STAMP_PROPERTY).is_some());
    }

    #[test]
    fn an_unstamped_or_foreign_artifact_is_refused() {
        // Written before stamping existed: parses, looks like a corpus, records
        // nothing about the build its terms came from.
        let unstamped = r#"{"analyzer":{"case_fold":"lowercase"},"documents":[]}"#;
        assert!(matches!(
            TfIdf::from_json(unstamped),
            Err(RestoreError::Stamp(crate::StampError::Missing))
        ));

        // Damaged bytes and a damaged stamp are different errors.
        assert!(matches!(
            TfIdf::from_json("{not json"),
            Err(RestoreError::Parse(_))
        ));
        let damaged = format!(
            r#"{{"{}":"17.0.0","analyzer":{{"case_fold":"lowercase"}},"documents":[]}}"#,
            crate::stamp::STAMP_PROPERTY
        );
        assert!(matches!(
            TfIdf::from_json(&damaged),
            Err(RestoreError::Stamp(crate::StampError::Malformed))
        ));

        // A foreign Unicode version, on a body this build could have written.
        let current = crate::ArtifactStamp::current();
        let (major, minor, update) = current.unicode;
        let foreign = format!(
            r#"{{"{}":{{"schema":{},"unicode":"{}.{minor}.{update}","lowercase":"{:016x}"}},"analyzer":{{"case_fold":"lowercase"}},"documents":[{{"terms":[["don't",1]]}}]}}"#,
            crate::stamp::STAMP_PROPERTY,
            current.schema,
            major - 1,
            current.lowercase.expect("this build stamps a fingerprint"),
        );
        assert!(matches!(
            TfIdf::from_json(&foreign),
            Err(RestoreError::Stamp(crate::StampError::Incompatible { .. }))
        ));
    }

    /// Bodies this build's stamp vouches for but that a corpus cannot mean.
    #[test]
    fn an_internally_inconsistent_artifact_is_refused() {
        let stamp = serde_json::to_string(&crate::ArtifactStamp::current().to_json())
            .expect("the stamp is plain JSON");
        let body = |analyzer: &str, documents: &str| {
            format!(
                r#"{{"{}":{stamp},"analyzer":{analyzer},"documents":{documents}}}"#,
                crate::stamp::STAMP_PROPERTY
            )
        };

        assert!(matches!(
            TfIdf::from_json(&body(
                r#"{"case_fold":"lowercase"}"#,
                r#"[{"terms":[["node",0]]}]"#
            )),
            Err(RestoreError::ZeroCount { document: 0, .. })
        ));
        assert!(matches!(
            TfIdf::from_json(&body(
                r#"{"case_fold":"lowercase"}"#,
                r#"[{"terms":[]},{"terms":[["node",1],["node",2]]}]"#
            )),
            Err(RestoreError::DuplicateTerm { document: 1, .. })
        ));
        assert!(matches!(
            TfIdf::from_json(&body(r#"{"case_fold":"casefold"}"#, "[]")),
            Err(RestoreError::UnknownCaseFold(_))
        ));
        // A body that is the right shape but the wrong types is a parse error,
        // not a corpus error.
        assert!(matches!(
            TfIdf::from_json(&body(r#"{"case_fold":"none"}"#, r#"[{"terms":"node"}]"#)),
            Err(RestoreError::Parse(_))
        ));
        // …and the shape that differs only by being consistent.
        let good = body(
            r#"{"case_fold":"lowercase"}"#,
            r#"[{"terms":[["node",2]]}]"#,
        );
        let restored = TfIdf::from_json(&good).expect("a consistent body");
        assert_eq!(restored.term_count(0, "node"), Some(2));
        assert_eq!(restored.document(0).expect("in range").total_terms(), 2);
    }

    #[test]
    fn is_send_and_sync() {
        const fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<TfIdf>();
        assert_send_sync::<Document>();
        assert_send_sync::<Analyzer>();
        assert_send_sync::<TermScore>();
        assert_send_sync::<DocumentScore>();
    }
}
