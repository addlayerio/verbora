//! `TfIdf` itself: the corpus, the idf cache, and the seven public operations.

use std::path::Path;
use std::sync::Arc;

#[cfg(feature = "parallel")]
use rayon::prelude::*;
use rustc_hash::FxHashMap;

use crate::document::{
    BuiltDocument, DocKey, Document, Interner, KEY_PROPERTY, RawDocument, ReadTarget, Slot, TermId,
};
use crate::encoding::Encoding;
use crate::globals;
use crate::value::{DynValue, JsonValue, Proto, array_index, prototype_member, write_json_string};

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

/// Something the reference throws.
///
/// The `Display` text is byte-identical to the reference message, because the
/// io_spec asserts on it (`Invalid encoding: foobar`) and because the parity
/// fixtures record thrown messages alongside returned values.
#[derive(Debug)]
pub enum TfIdfError {
    /// A property read on `undefined`: an out-of-range document index, or an
    /// instance built from `new TfIdf({})` whose `documents` is `undefined`.
    UndefinedRead(String),
    /// A property read on `null`: a `null` document in the corpus.
    NullRead(String),
    /// `Buffer.isEncoding` rejected the name.
    InvalidEncoding(String),
    /// `fs.readFileSync` failed.
    Io(std::io::Error),
}

impl std::fmt::Display for TfIdfError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UndefinedRead(p) => {
                write!(f, "Cannot read properties of undefined (reading '{p}')")
            }
            Self::NullRead(p) => write!(f, "Cannot read properties of null (reading '{p}')"),
            Self::InvalidEncoding(e) => write!(f, "Invalid encoding: {e}"),
            Self::Io(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for TfIdfError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(e) => Some(e),
            _ => None,
        }
    }
}

impl From<std::io::Error> for TfIdfError {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e)
    }
}

// ---------------------------------------------------------------------------
// Inputs
// ---------------------------------------------------------------------------

/// What `addDocument` was handed.
///
/// The three variants are not stylistic: they take *different code paths* in
/// `buildDocument`, and the difference is observable.
#[derive(Debug, Clone)]
pub enum DocumentInput<'a> {
    /// A string: lowercased, tokenized with the process-global tokenizer, and
    /// **stop-word filtered**.
    Text(&'a str),
    /// An already-tokenized list: used verbatim. **Not** lowercased and **not**
    /// filtered, so `["The", "the", "THE"]` produces three distinct terms.
    Tokens(&'a [&'a str]),
    /// Anything else: stored unchanged. No `__key` is attached and no counting
    /// happens, but the slot still counts towards every idf's denominator.
    Raw(JsonValue),
}

/// What `tfidf`/`tfidfs` were handed.
///
/// Unlike `addDocument`, **no stop-word filtering happens here**, so querying
/// `"the"` computes and caches an idf for a term that a string-built document
/// can never contain. Sharing one helper between the two would be the natural
/// refactor and would change results.
#[derive(Debug, Clone, Copy)]
pub enum Terms<'a> {
    /// A string: lowercased and tokenized with the process-global tokenizer.
    Text(&'a str),
    /// An already-tokenized list, used verbatim.
    Tokens(&'a [&'a str]),
}

/// Query terms after the tokenizer has run, if it needed to.
enum Resolved<'a> {
    Owned(Vec<String>),
    Borrowed(&'a [&'a str]),
}

impl Resolved<'_> {
    fn iter(&self) -> impl Iterator<Item = &str> {
        match self {
            Self::Owned(v) => itertools_either::Left(v.iter().map(String::as_str)),
            Self::Borrowed(v) => itertools_either::Right(v.iter().copied()),
        }
    }
}

/// A two-variant iterator, so `Resolved::iter` needs no allocation and no `dyn`.
mod itertools_either {
    pub use Either::{Left, Right};

    /// One of two iterators with the same item type.
    pub enum Either<L, R> {
        /// The left iterator.
        Left(L),
        /// The right iterator.
        Right(R),
    }

    impl<T, L: Iterator<Item = T>, R: Iterator<Item = T>> Iterator for Either<L, R> {
        type Item = T;
        fn next(&mut self) -> Option<T> {
            match self {
                Self::Left(l) => l.next(),
                Self::Right(r) => r.next(),
            }
        }
    }
}

/// One row of [`TfIdf::list_terms`].
#[derive(Debug, Clone)]
pub struct TermScore {
    /// The term.
    pub term: String,
    /// `TfIdf.tf(term, document)` — a raw property read, so it is not always a
    /// number: a deserialized corpus can hand back a string.
    pub tf: DynValue,
    /// `idf(term)`, which on a prototype-backed cache can be a function.
    pub idf: DynValue,
    /// `tfidf([term], d)`, always a number (possibly `NaN`).
    pub tfidf: f64,
}

impl TermScore {
    /// The term frequency coerced to a number, as every arithmetic use does.
    pub fn tf_as_f64(&self) -> f64 {
        self.tf.to_number()
    }

    /// The inverse document frequency coerced to a number.
    pub fn idf_as_f64(&self) -> f64 {
        self.idf.to_number()
    }
}

// ---------------------------------------------------------------------------
// The idf cache
// ---------------------------------------------------------------------------

/// `this._idfCache`.
///
/// The prototype flag is not bookkeeping: the constructor and `addFileSync`
/// install a plain `{}`, while `addDocument` and `removeDocument` install
/// `Object.create(null)`. Because the cache probe is a truthiness test rather
/// than a `hasOwnProperty` check, the first kind answers `idf('toString')` with
/// `Object.prototype.toString` — a function — and `tfidf(['toString'], 0)` is
/// then `NaN`. A plain `HashMap` silently "fixes" that.
#[derive(Debug, Clone)]
struct IdfCache {
    entries: Vec<(Arc<str>, f64)>,
    index: FxHashMap<Arc<str>, u32>,
    prototype: bool,
}

impl IdfCache {
    /// `{}` — backed by `Object.prototype`.
    fn plain() -> Self {
        Self {
            entries: Vec::new(),
            index: FxHashMap::default(),
            prototype: true,
        }
    }

    /// `Object.create(null)`.
    fn null_prototype() -> Self {
        Self {
            entries: Vec::new(),
            index: FxHashMap::default(),
            prototype: false,
        }
    }

    fn own(&self, term: &str) -> Option<f64> {
        self.index.get(term).map(|i| self.entries[*i as usize].1)
    }

    fn set(&mut self, term: &str, value: f64) {
        if self.prototype && term == crate::document::PROTO_PROPERTY {
            // `cache.__proto__ = <number>` invokes the setter, which ignores
            // anything that is not an object — so nothing is stored, and the
            // next probe finds `Object.prototype` again.
            return;
        }
        if let Some(i) = self.index.get(term) {
            self.entries[*i as usize].1 = value;
            return;
        }
        let key: Arc<str> = Arc::from(term);
        let i = u32::try_from(self.entries.len()).expect("cache holds < u32::MAX terms");
        self.entries.push((Arc::clone(&key), value));
        self.index.insert(key, i);
    }

    /// The keys in `for…in` order: array-index-like first, then insertion order.
    fn ordered_keys(&self) -> Vec<Arc<str>> {
        let mut indexed: Vec<(u32, usize)> = Vec::new();
        let mut rest: Vec<usize> = Vec::new();
        for (i, (k, _)) in self.entries.iter().enumerate() {
            match array_index(k) {
                Some(n) => indexed.push((n, i)),
                None => rest.push(i),
            }
        }
        indexed.sort_unstable();
        indexed
            .into_iter()
            .map(|(_, i)| i)
            .chain(rest)
            .map(|i| Arc::clone(&self.entries[i].0))
            .collect()
    }
}

// ---------------------------------------------------------------------------
// TfIdf
// ---------------------------------------------------------------------------

/// A TF-IDF corpus, matching the reference's `TfIdf`.
///
/// ```
/// use verbora_tfidf::{DocKey, DocumentInput, TfIdf, Terms};
///
/// let mut tfidf = TfIdf::new();
/// tfidf.add_document(DocumentInput::Text("this document is about node."), DocKey::Undefined, false).unwrap();
/// tfidf.add_document(DocumentInput::Text("this document is about ruby."), DocKey::Undefined, false).unwrap();
/// // 'node' appears in one of two documents.
/// assert_eq!(tfidf.idf("node").unwrap(), 1.0 + (2.0f64 / 2.0).ln());
/// assert_eq!(tfidf.tfidf(Terms::Text("ruby"), 0).unwrap(), 0.0);
/// ```
///
/// # State that is not on the instance
///
/// The tokenizer and the stop-word list are **process-global** — see
/// [`crate::globals`]. `TfIdf::set_tokenizer` is an associated function here
/// rather than a method precisely so the sharing is visible at the call site;
/// in the reference it is an instance method that mutates a module variable.
#[derive(Debug, Clone)]
pub struct TfIdf {
    /// `None` reproduces `new TfIdf({})`, where `this.documents` is `undefined`
    /// and every later call throws.
    documents: Option<Vec<Document>>,
    interner: Interner,
    cache: IdfCache,
    /// Term id → how many *built* documents hold a count greater than zero.
    ///
    /// The reference re-scans the whole corpus on every cache miss. Keeping this
    /// incrementally makes a miss O(1) without changing a single result,
    /// because cache invalidation timing is untouched.
    built_df: FxHashMap<TermId, u32>,
    /// Indices of documents that are **not** built term maps, and so have to be
    /// scanned the slow way. Empty for every corpus built through
    /// `add_document`/`add_file_sync` with string or token input.
    exotic: Vec<usize>,
    /// Reusable buffers for the text-ingestion fast path — see `fast_build`.
    /// Zeroed between calls; carried on the instance so a corpus pays their
    /// allocation once, not once per document.
    scratch: crate::fast_build::FastScratch,
}

impl Default for TfIdf {
    fn default() -> Self {
        Self::new()
    }
}

impl TfIdf {
    /// An empty corpus — `new TfIdf()`.
    pub fn new() -> Self {
        Self {
            documents: Some(Vec::new()),
            interner: Interner::default(),
            cache: IdfCache::plain(),
            built_df: FxHashMap::default(),
            exotic: Vec::new(),
            scratch: crate::fast_build::FastScratch::default(),
        }
    }

    /// The deserializing constructor — `new TfIdf(deserialized)`.
    ///
    /// Reproduces the reference's total absence of validation:
    ///
    /// * a falsy argument (`0`, `''`, `null`, `false`, `NaN`) takes the
    ///   empty-corpus branch;
    /// * a truthy argument without a `documents` property leaves `documents`
    ///   **undefined**, so every later call throws;
    /// * the documents are taken as they are — objects, numbers and `null` all
    ///   occupy a corpus slot and all count towards the idf denominator.
    pub fn from_value(deserialized: &JsonValue) -> Self {
        if !deserialized.is_truthy() {
            return Self::new();
        }
        let documents = match deserialized.own("documents") {
            Some(JsonValue::Arr(items)) => Some(
                items
                    .iter()
                    .map(|v| Document::Raw(RawDocument::new(v.clone())))
                    .collect::<Vec<_>>(),
            ),
            // A `documents` that is not an array is as unusable as an absent
            // one: `.push`/`.length`/`.reduce` all fail. Both map to `None`.
            _ => None,
        };
        let exotic = documents
            .as_ref()
            .map(|d| (0..d.len()).collect())
            .unwrap_or_default();
        Self {
            documents,
            interner: Interner::default(),
            cache: IdfCache::plain(),
            built_df: FxHashMap::default(),
            exotic,
            scratch: crate::fast_build::FastScratch::default(),
        }
    }

    /// Parses a serialized corpus — `new TfIdf(JSON.parse(s))`.
    ///
    /// # Errors
    ///
    /// Returns the parse error if `json` is not valid JSON.
    pub fn from_json(json: &str) -> Result<Self, serde_json::Error> {
        let value: JsonValue = serde_json::from_str(json)?;
        Ok(Self::from_owned_value(value))
    }

    /// Same behaviour as [`Self::from_value`], but for a caller that owns the
    /// parsed value outright.
    ///
    /// `from_value` takes `&JsonValue` because most of its callers do not own
    /// their argument, so it clones each document out of the borrow. A freshly
    /// parsed [`Self::from_json`] value has no other owner, so this moves each
    /// document's data into its [`RawDocument`] instead of cloning it. See
    /// `query/from_json` in `benches/tfidf.rs` / `docs/PERFORMANCE.md` for the
    /// measured effect.
    fn from_owned_value(deserialized: JsonValue) -> Self {
        if !deserialized.is_truthy() {
            return Self::new();
        }
        let documents = match deserialized.into_own("documents") {
            Some(JsonValue::Arr(items)) => Some(
                items
                    .into_iter()
                    .map(|v| Document::Raw(RawDocument::new(v)))
                    .collect::<Vec<_>>(),
            ),
            // A `documents` that is not an array is as unusable as an absent
            // one: `.push`/`.length`/`.reduce` all fail. Both map to `None`.
            _ => None,
        };
        let exotic = documents
            .as_ref()
            .map(|d| (0..d.len()).collect())
            .unwrap_or_default();
        Self {
            documents,
            interner: Interner::default(),
            cache: IdfCache::plain(),
            built_df: FxHashMap::default(),
            exotic,
            scratch: crate::fast_build::FastScratch::default(),
        }
    }

    // -- global state -------------------------------------------------------

    /// Installs a **process-global** tokenizer — `TfIdf#setTokenizer`.
    ///
    /// Every `TfIdf` in the process is affected, including ones created before
    /// this call. See [`crate::globals::set_tokenizer`].
    pub fn set_tokenizer(tokenizer: Arc<dyn globals::TfIdfTokenizer>) {
        globals::set_tokenizer(tokenizer);
    }

    /// Installs a **process-global** stop-word list — `TfIdf#setStopwords`.
    ///
    /// Returns `false` (changing nothing) when the list is rejected, matching
    /// The reference's validation exactly.
    pub fn set_stopwords(list: &globals::StopwordList) -> bool {
        globals::set_stopwords(list)
    }

    // -- reading ------------------------------------------------------------

    /// The corpus, or `None` if this instance was built from a deserialized
    /// object with no `documents`.
    pub fn documents(&self) -> Option<&[Document]> {
        self.documents.as_deref()
    }

    /// The interner the documents' term ids refer to.
    pub fn interner(&self) -> &Interner {
        &self.interner
    }

    /// The cached idfs in `for…in` order — the map `JSON.stringify` emits.
    pub fn idf_cache(&self) -> Vec<(String, f64)> {
        self.cache
            .ordered_keys()
            .into_iter()
            .map(|k| {
                let v = self.cache.own(&k).unwrap_or(f64::NAN);
                (k.to_string(), v)
            })
            .collect()
    }

    /// Whether the idf cache is the prototype-backed `{}` rather than
    /// `Object.create(null)`.
    ///
    /// Exposed because it is the only thing that distinguishes the two, and it
    /// changes what `idf` returns for `toString`, `constructor` and friends.
    pub fn idf_cache_is_prototype_backed(&self) -> bool {
        self.cache.prototype
    }

    fn docs(&self, reading: &str) -> Result<&[Document], TfIdfError> {
        self.documents
            .as_deref()
            .ok_or_else(|| TfIdfError::UndefinedRead(reading.to_owned()))
    }

    // -- tf -----------------------------------------------------------------

    /// `TfIdf.tf(term, document)` — a bare property read with the reference truthiness.
    ///
    /// It is not a count: it returns whatever the property held, falling back to
    /// `0` for anything falsy. A deserialized `-2` comes straight back, and a
    /// document that inherits from `Object.prototype` returns a *function* for
    /// `toString`.
    ///
    /// # Errors
    ///
    /// Returns [`TfIdfError::NullRead`] when `document` is `null`.
    pub fn tf(
        term: &str,
        document: &Document,
        interner: &Interner,
    ) -> Result<DynValue, TfIdfError> {
        let value = document
            .get(term, interner)
            .map_err(|t| read_error(t, term))?;
        Ok(if value.is_truthy() {
            value
        } else {
            DynValue::Num(0.0)
        })
    }

    /// `TfIdf.tf(term, this.documents[d])`.
    ///
    /// # Errors
    ///
    /// Returns [`TfIdfError::UndefinedRead`] when `d` is out of range, which is
    /// what `documents[d]` being `undefined` does in the reference.
    pub fn tf_at(&self, term: &str, d: usize) -> Result<DynValue, TfIdfError> {
        let docs = self.docs(term)?;
        let doc = docs
            .get(d)
            .ok_or_else(|| TfIdfError::UndefinedRead(term.to_owned()))?;
        Self::tf(term, doc, &self.interner)
    }

    // -- idf ----------------------------------------------------------------

    /// The inverse document frequency of `term`, as a number.
    ///
    /// # Errors
    ///
    /// Propagates the `TypeError`s a `null` document or an undefined `documents`
    /// array produce.
    pub fn idf(&mut self, term: &str) -> Result<f64, TfIdfError> {
        Ok(self.idf_value(term, false)?.to_number())
    }

    /// The exact the reference return value of `idf(term, force)`.
    ///
    /// Usually a [`DynValue::Num`]. On the prototype-backed cache installed by the
    /// constructor and by `add_file_sync`, a term named after an
    /// `Object.prototype` member short-circuits the truthiness probe and comes
    /// back as [`DynValue::Function`] — which is why `tfidf(["toString"], 0)` is
    /// `NaN` in both implementations.
    ///
    /// `force` is the reference's `force === true`: any other value reads the
    /// cache.
    ///
    /// # Errors
    ///
    /// Propagates the `TypeError`s a `null` document or an undefined `documents`
    /// array produce.
    pub fn idf_value(&mut self, term: &str, force: bool) -> Result<DynValue, TfIdfError> {
        if !force {
            match self.cache.own(term) {
                // Truthiness, not presence: a cached 0 or NaN would recompute.
                Some(v) if v != 0.0 && !v.is_nan() => return Ok(DynValue::Num(v)),
                Some(_) => {}
                None => {
                    if self.cache.prototype
                        && let Some(inherited) = prototype_member(Proto::Object, term)
                    {
                        return Ok(inherited);
                    }
                }
            }
        }

        let docs_with_term = self.docs_with_term(term)?;
        #[expect(clippy::cast_precision_loss, reason = "corpora are far below 2^53")]
        let total = self.docs(term)?.len() as f64;
        // Two hazards in one line. The division happens INSIDE the logarithm,
        // exactly as written — the algebraically equal `ln(n) - ln(1 + d)`
        // differs in the last bit. And the logarithm is the reference engine's, not the platform
        // libm's: see `crate::mathlog` for why `f64::ln` is the wrong function.
        let idf = 1.0 + crate::mathlog::math_log(total / (1.0 + f64::from(docs_with_term)));
        self.cache.set(term, idf);
        Ok(DynValue::Num(idf))
    }

    /// `documents.reduce((n, doc) => n + (documentHasTerm(term, doc) ? 1 : 0), 0)`.
    fn docs_with_term(&self, term: &str) -> Result<u32, TfIdfError> {
        let docs = self.docs("reduce")?;

        // Fast path: every document is a built term map, so the incremental
        // count is exact. `__key` is excluded because it lives outside the
        // count index and a numeric key really does satisfy `value > 0`.
        if self.exotic.is_empty() && term != KEY_PROPERTY {
            return Ok(self
                .interner
                .lookup(term)
                .and_then(|id| self.built_df.get(&id).copied())
                .unwrap_or(0));
        }

        let mut count = 0;
        for doc in docs {
            let value = doc
                .get(term, &self.interner)
                .map_err(|t| read_error(t, term))?;
            if value.counts_as_present() {
                count += 1;
            }
        }
        Ok(count)
    }

    // -- mutation -----------------------------------------------------------

    /// Adds a document — `addDocument(document, key, restoreCache)`.
    ///
    /// `restore_cache` recomputes every currently cached idf in place instead of
    /// discarding the cache. Note the asymmetry with [`Self::add_file_sync`]:
    /// discarding here installs `Object.create(null)`, there a plain `{}`.
    ///
    /// # Errors
    ///
    /// Returns [`TfIdfError::UndefinedRead`] (`push`) when `documents` is
    /// undefined, and propagates whatever a `restore_cache` refresh throws.
    pub fn add_document(
        &mut self,
        document: DocumentInput<'_>,
        key: DocKey,
        restore_cache: bool,
    ) -> Result<(), TfIdfError> {
        let built = self.build_document(document, key);
        self.push(built, "push")?;
        if restore_cache {
            self.restore_cache()?;
        } else {
            self.cache = IdfCache::null_prototype();
        }
        Ok(())
    }

    /// Adds a document read from disk — `addFileSync(path, encoding, key, …)`.
    ///
    /// A falsy `encoding` becomes `utf8`; the io_spec passes `null` explicitly.
    /// The encoding is **not** just validation: reading a UTF-8 file as `base64`
    /// tokenizes the base64 *text*.
    ///
    /// # Errors
    ///
    /// [`TfIdfError::InvalidEncoding`] when `Buffer.isEncoding` would reject the
    /// name, [`TfIdfError::Io`] when the read fails, and the same `push` error
    /// as [`Self::add_document`].
    pub fn add_file_sync(
        &mut self,
        path: impl AsRef<Path>,
        encoding: Option<&str>,
        key: DocKey,
        restore_cache: bool,
    ) -> Result<(), TfIdfError> {
        // `if (!encoding) encoding = 'utf8'` — every falsy value, `''` included.
        let name = encoding.filter(|e| !e.is_empty()).unwrap_or("utf8");
        let encoding =
            Encoding::parse(name).ok_or_else(|| TfIdfError::InvalidEncoding(name.to_owned()))?;
        // The read happens before the push, so a missing `documents` still
        // touches the filesystem first.
        let bytes = std::fs::read(path)?;
        let text = encoding.decode(&bytes);
        let built = self.build_document(DocumentInput::Text(&text), key);
        self.push(built, "push")?;
        if restore_cache {
            self.restore_cache()?;
        } else {
            // NOT `Object.create(null)`. The asymmetry with addDocument is
            // observable: `idf('toString')` returns a function after this.
            self.cache = IdfCache::plain();
        }
        Ok(())
    }

    /// Adds many text documents at once, tokenizing them in parallel over
    /// Rayon's global thread pool — gated behind the `parallel` feature.
    ///
    /// # Why this exists, and why it is shaped the way it is
    ///
    /// [`Self::add_document`] cannot be fanned out with a naive
    /// `docs.par_iter().for_each(|d| corpus.add_document(d, ..))`: it takes
    /// `&mut self`, and every call mutates three pieces of *shared* corpus
    /// state — [`Interner`], the incremental document-frequency table, and the
    /// idf cache's identity. Wrapping the corpus in a `Mutex` would compile,
    /// but it would serialize exactly the work a parallel version exists to
    /// speed up, for a net loss once lock contention is added on top.
    ///
    /// The actual cost of ingesting a **text** document is concentrated in
    /// [`lowercase_units`] and the process-global tokenizer
    /// ([`globals::tokenize_global`]) — both pure functions of the input text
    /// and the (thread-safely-read) global tokenizer, touching no corpus
    /// state at all. This method fans out exactly that part:
    ///
    /// 1. **Parallel** — every `(text, key)` pair's text is lowercased and
    ///    tokenized independently, via the *exact same* [`lowercase_units`] and
    ///    [`globals::tokenize_global`] calls the reference `Text` ingestion
    ///    loop makes. Nothing here re-derives tokenizing or lowercasing
    ///    rules; both are called, not reimplemented.
    /// 2. **Sequential** — the tokenized documents are replayed, in original
    ///    order, through the reference per-token loop:
    ///    [`globals::is_stopword`], [`Interner::intern`],
    ///    [`BuiltDocument::observe`], then [`Self::push`]. This is the part
    ///    that touches the shared interner and document-frequency table —
    ///    same calls, same order, same result as calling
    ///    [`Self::add_document`] once per document. (With the default
    ///    tokenizer, `add_document` itself runs `fast_build`'s fused scan
    ///    instead of this loop; the two are differentially verified to
    ///    produce byte-exact state, so the replay remains equivalent.)
    ///
    /// Because step 2 replays step 1's output in the original order through
    /// unmodified sequential primitives, this method is provably equivalent
    /// to calling `add_document(DocumentInput::Text(text), key, false)` once
    /// per pair, in order — see `tests/parallel.rs` for a battery of
    /// sequential-vs-parallel parity assertions, including every pathological
    /// input `src/tfidf.rs`'s own test suite already knows about.
    ///
    /// # Scope: `Text` only, and always `restore_cache: false`
    ///
    /// This method only accepts text documents — the [`DocumentInput::Text`]
    /// shape, the one whose ingestion cost this method exists to parallelize.
    /// [`DocumentInput::Tokens`] has no tokenizing to parallelize (it is used
    /// verbatim) and [`DocumentInput::Raw`] does no processing at all; add
    /// those with the ordinary sequential [`Self::add_document`], before or
    /// after a batch call.
    ///
    /// There is no `restore_cache` parameter: this method always behaves like
    /// `restore_cache: false`, discarding the idf cache and reinstalling
    /// `Object.create(null)` once, after every document in the batch has been
    /// pushed — matching what `N` sequential `add_document(.., false)` calls
    /// leave behind, since each of them replaces the cache unconditionally
    /// with the same fresh, empty, null-prototype cache. Supporting
    /// `restore_cache: true` here would not be a matter of extra plumbing: a
    /// stale-cache recompute that fails partway (a term whose recompute
    /// throws, because a *pre-existing* `null`/raw document in the corpus
    /// makes `docs_with_term` throw for that term) would need to leave the
    /// corpus in exactly the state an `N`-call sequential loop would have
    /// stopped at on the same failure — i.e. only the documents pushed
    /// *before* the failing recompute. This method pushes every document
    /// before it ever touches the cache, so it cannot reproduce that partial
    /// state. Rather than risk that narrow but real divergence, `restore_cache
    /// : true` is simply out of scope: call [`Self::add_document`] in a loop
    /// if you need it.
    ///
    /// # Errors
    ///
    /// Returns [`TfIdfError::UndefinedRead`] (`push`) under the same
    /// condition [`Self::add_document`] does: an instance built from `new
    /// TfIdf({})`-equivalent input, whose `documents` is `None`. Because that
    /// condition is a property of the whole corpus, not of any one document,
    /// it is detected on the first document and none are pushed — the same
    /// outcome an `N`-call sequential loop stopping at its first `?` would
    /// leave behind.
    ///
    /// # Panics
    ///
    /// Never on its own account. A panic inside a custom
    /// [`globals::TfIdfTokenizer`] installed via [`Self::set_tokenizer`]
    /// propagates out of the offending Rayon worker per Rayon's own
    /// panic-propagation rules — since that happens during the *parallel*
    /// phase, before any document is pushed, the corpus is left completely
    /// unchanged, which is not a guarantee the sequential loop makes (it would
    /// have already pushed every document before the panicking one).
    ///
    /// # What it costs
    ///
    /// One `Vec<Vec<String>>` sized to `documents.len()` for the tokenized
    /// intermediate — one `String` allocation per token, even in the default
    /// tokenizer's normal zero-per-token-allocation path
    /// ([`globals::GlobalTokens::Default`] borrows out of the lowered text),
    /// because tokens have to outlive the parallel closure that produced them
    /// to reach the sequential replay. That is the genuine price of this
    /// method versus the sequential loop, paid in exchange for running the
    /// lowercasing + tokenizing work — the dominant cost per
    /// `crates/verbora-tfidf/benches/tfidf.rs`'s own `build` group — on more
    /// than one core. Granularity is one Rayon task per document (`par_iter`
    /// over the input slice); there is no chunk-size parameter, because a
    /// text document is rarely as cheap as the single short words
    /// `verbora_phonetics::par_encode_batch` chunks for.
    ///
    /// # When to reach for it, and what it actually buys you
    ///
    /// `benches/tfidf.rs`'s `parallel_batch` group (feature = "parallel")
    /// measures two corpus shapes on one 32-core machine: `few_large` (8/64/256
    /// documents, each close to the full 167 kB source article, the same
    /// documents `idf_cold` uses) and `many_small` (128/1,024/8,192 documents
    /// of a fixed ~1.2 kB each — closer to "one review, one ticket"). The win
    /// is real but modest, not the near-linear-in-core-count speedup the core
    /// count alone might suggest:
    ///
    /// | Shape | N | sequential | `par_add_documents_batch` | Δ |
    /// |---|---:|---:|---:|---:|
    /// | `few_large` | 8 | 18.4 ms | 17.5 ms | ~5% faster |
    /// | `few_large` | 64 | 141.1 ms | 130.9 ms | ~7% faster |
    /// | `few_large` | 256 | 560.5 ms | 514.4 ms | ~8% faster |
    /// | `many_small` | 128 | 3.46 ms | 3.69 ms | ~7% **slower** |
    /// | `many_small` | 1,024 | 25.6 ms | 23.5 ms | ~8% faster |
    /// | `many_small` | 8,192 | 211.6 ms | 183.2 ms | ~13% faster |
    ///
    /// Two things follow from those numbers, not from theory:
    ///
    /// * **The sequential interning/counting replay is not free.** If it were
    ///   negligible next to tokenizing, 32 cores tokenizing in parallel would
    ///   show a far larger win than the ~5–13% measured here — this is
    ///   Amdahl's law made concrete, not a bug. The interner and
    ///   `BuiltDocument` index lookups this method deliberately leaves
    ///   sequential are a real, non-negligible share of total ingestion time
    ///   for these document sizes.
    /// * **Below roughly a thousand small documents (or a few dozen large
    ///   ones), the sequential loop can win outright** — `many_small/128` is
    ///   measurably *slower* in parallel, the fork-join/scheduling cost not
    ///   yet amortized. Reach for this method when ingesting a genuinely
    ///   large batch in one call; for a handful of documents, or documents
    ///   arriving one at a time, `add_document` in a loop is simpler and at
    ///   least as fast.
    ///
    /// # Examples
    ///
    /// ```
    /// # #[cfg(feature = "parallel")]
    /// # {
    /// use verbora_tfidf::{DocKey, DocumentInput, TfIdf};
    ///
    /// let docs = [
    ///     ("this document is about node.", DocKey::Num(0.0)),
    ///     ("this document is about ruby.", DocKey::Num(1.0)),
    /// ];
    ///
    /// let mut parallel = TfIdf::new();
    /// parallel.par_add_documents_batch(&docs).unwrap();
    ///
    /// let mut sequential = TfIdf::new();
    /// for (text, key) in docs {
    ///     sequential
    ///         .add_document(DocumentInput::Text(text), key, false)
    ///         .unwrap();
    /// }
    ///
    /// assert_eq!(parallel.to_json(), sequential.to_json());
    /// # }
    /// ```
    #[cfg(feature = "parallel")]
    pub fn par_add_documents_batch(
        &mut self,
        documents: &[(&str, DocKey)],
    ) -> Result<(), TfIdfError> {
        if documents.is_empty() {
            // Zero iterations of the sequential loop touch nothing — not even
            // the cache — so this must not either.
            return Ok(());
        }

        // Parallel phase: pure per-document computation, no corpus state
        // touched. Identical to the lowercasing + tokenizing `build_document`
        // already performs for `DocumentInput::Text`, just computed ahead of
        // time instead of inline.
        let tokenized: Vec<Vec<String>> = documents
            .par_iter()
            .map(|(text, _)| {
                let lowered = lowercase_units(text);
                let mut tokens = Vec::new();
                globals::tokenize_global(&lowered).for_each(|t| tokens.push(t.to_owned()));
                tokens
            })
            .collect();

        // Sequential phase: replay `build_document`'s own per-token loop and
        // `push`, in original order — the exact same `Interner::intern` /
        // `BuiltDocument::observe` calls `add_document` makes, fed
        // pre-tokenized text instead of tokenizing inline.
        for ((_, key), tokens) in documents.iter().zip(tokenized) {
            let mut doc = BuiltDocument::new(key.clone());
            for term in &tokens {
                let filtered = globals::is_stopword(term);
                let id = self.interner.intern(term);
                doc.observe(id, term, filtered, &self.interner);
            }
            self.push(Document::Built(doc), "push")?;
        }

        // Same unconditional identity swap every `restore_cache: false`
        // `add_document`/`add_file_sync`... call performs — see that method's
        // doc comment for why `Object.create(null)`, not `{}`.
        self.cache = IdfCache::null_prototype();
        Ok(())
    }

    /// Removes the first document whose key is `===` to `key`.
    ///
    /// Returns whether one was found. Object keys compare by **identity**, so a
    /// structurally identical copy will not match; documents stored verbatim
    /// have no `__key` at all and are matched by [`DocKey::Undefined`].
    ///
    /// # Errors
    ///
    /// Returns [`TfIdfError::UndefinedRead`] (`findIndex`) when `documents` is
    /// undefined, and [`TfIdfError::NullRead`] when a `null` document is reached
    /// before a match.
    pub fn remove_document(&mut self, key: &DocKey) -> Result<bool, TfIdfError> {
        let docs = self
            .documents
            .as_ref()
            .ok_or_else(|| TfIdfError::UndefinedRead("findIndex".to_owned()))?;

        let mut found = None;
        for (i, doc) in docs.iter().enumerate() {
            let doc_key = doc.remove_key().map_err(|t| read_error(t, KEY_PROPERTY))?;
            if doc_key.strict_eq(key) {
                found = Some(i);
                break;
            }
        }
        let Some(index) = found else { return Ok(false) };

        let documents = self.documents.as_mut().expect("checked above");
        let removed = documents.remove(index);
        if let Document::Built(doc) = &removed {
            for (id, count) in doc.entries() {
                if *count > 0.0
                    && let Some(n) = self.built_df.get_mut(id)
                {
                    *n -= 1;
                }
            }
        }
        self.rebuild_exotic();
        self.cache = IdfCache::null_prototype();
        Ok(true)
    }

    // -- queries ------------------------------------------------------------

    /// `tfidf(terms, d)` — the sum of `tf * idf` over the query terms.
    ///
    /// Accumulation is strictly left to right over the token list, because the
    /// the reference specs compare with `toBe` and a different summation order changes the
    /// last bit. Only `+Infinity` is clamped to `0`; `-Infinity` (which an empty
    /// corpus produces) and `NaN` are not.
    ///
    /// # Errors
    ///
    /// Propagates the `TypeError` an out-of-range `d` produces — but only once
    /// there is at least one term, since an empty query never reads the
    /// document.
    pub fn tfidf(&mut self, terms: Terms<'_>, d: usize) -> Result<f64, TfIdfError> {
        let resolved = Self::resolve(terms);
        let mut value = 0.0;
        for term in resolved.iter() {
            // Evaluation order matters: `idf` runs before `tf`, so on an empty
            // corpus the -Infinity is produced before the out-of-range throw.
            let idf = self.idf_value(term, false)?;
            let idf = match idf {
                DynValue::Num(n) if n == f64::INFINITY => 0.0,
                other => other.to_number(),
            };
            let tf = self.tf_at(term, d)?;
            value += tf.to_number() * idf;
        }
        Ok(value)
    }

    /// `tfidfs(terms)` — [`Self::tfidf`] against every document, in order.
    ///
    /// # Errors
    ///
    /// Propagates whatever [`Self::tfidf`] throws for any document.
    pub fn tfidfs(&mut self, terms: Terms<'_>) -> Result<Vec<f64>, TfIdfError> {
        self.tfidfs_with(terms, |_, _, _| {})
    }

    /// `tfidfs(terms, callback)`.
    ///
    /// The callback receives `(index, measure, key)` synchronously in ascending
    /// index order, and its return value is ignored. `key` is the document's
    /// `__key` property read back out of the map, so a document stored verbatim
    /// yields [`DynValue::Undefined`].
    ///
    /// # Errors
    ///
    /// Propagates whatever [`Self::tfidf`] throws, and the `length` read when
    /// `documents` is undefined.
    pub fn tfidfs_with(
        &mut self,
        terms: Terms<'_>,
        mut callback: impl FnMut(usize, f64, DynValue),
    ) -> Result<Vec<f64>, TfIdfError> {
        // `new Array(this.documents.length)` is the first thing that runs.
        let len = self.docs("length")?.len();
        let mut out = Vec::with_capacity(len);
        for i in 0..len {
            let measure = self.tfidf(terms, i)?;
            out.push(measure);
            let key = self.documents.as_ref().expect("checked above")[i]
                .key_value(&self.interner)
                .map_err(|t| read_error(t, KEY_PROPERTY))?;
            callback(i, measure, key);
        }
        Ok(out)
    }

    /// `listTerms(d)` — every term of a document, ranked by tf-idf descending.
    ///
    /// Two orderings are load-bearing. The terms are visited in the reference
    /// `for…in` order (array-index-like keys first, ascending, then insertion
    /// order), and the sort is **stable**, so equal scores keep that order. A
    /// `BTreeMap` (lexicographic) or `sort_unstable_by` would both diverge.
    ///
    /// An out-of-range `d` yields an empty list rather than throwing, because a
    /// `for…in` over `undefined` iterates zero times.
    ///
    /// # Errors
    ///
    /// Propagates what the per-term `idf`/`tfidf` calls throw.
    pub fn list_terms(&mut self, d: usize) -> Result<Vec<TermScore>, TfIdfError> {
        let names = {
            let docs = self.docs(&d.to_string())?;
            match docs.get(d) {
                Some(doc) => doc.for_in_keys(&self.interner),
                None => Vec::new(),
            }
        };

        let mut terms = Vec::with_capacity(names.len());
        for term in names {
            if term == KEY_PROPERTY {
                continue;
            }
            let tf = self.tf_at(&term, d)?;
            let idf = self.idf_value(&term, false)?;
            let one = [term.as_str()];
            let tfidf = self.tfidf(Terms::Tokens(&one), d)?;
            terms.push(TermScore {
                term,
                tf,
                idf,
                tfidf,
            });
        }

        // `sort((x, y) => y.tfidf - x.tfidf)`, run through the reference engine's own algorithm.
        // With finite scores any stable sort would do; with a NaN score the
        // comparator stops being transitive and the permutation becomes a
        // property of the reference engine's TimSort. See `crate::comparator_sort`.
        let mut order: Vec<(u32, f64)> = terms
            .iter()
            .enumerate()
            .map(|(i, t)| (u32::try_from(i).unwrap_or(u32::MAX), t.tfidf))
            .collect();
        crate::comparator_sort::sort_by(&mut order, |a, b| if b.1 - a.1 < 0.0 { -1 } else { 1 });
        let mut slots: Vec<Option<TermScore>> = terms.into_iter().map(Some).collect();
        Ok(order
            .into_iter()
            .map(|(i, _)| slots[i as usize].take().expect("each index appears once"))
            .collect())
    }

    // -- serialization ------------------------------------------------------

    /// `JSON.stringify(tfidf)`.
    ///
    /// Reproduces the exact string the io_spec asserts, including `__key` coming
    /// first inside each document and a `__key` of `undefined` being **omitted**
    /// rather than emitted as `null`.
    pub fn to_json(&self) -> String {
        let mut out = String::from("{");
        if let Some(documents) = &self.documents {
            out.push_str("\"documents\":[");
            for (i, doc) in documents.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                self.write_document_json(doc, &mut out);
            }
            out.push_str("],");
        }
        out.push_str("\"_idfCache\":{");
        for (i, key) in self.cache.ordered_keys().into_iter().enumerate() {
            if i > 0 {
                out.push(',');
            }
            write_json_string(&key, &mut out);
            out.push(':');
            let v = self.cache.own(&key).unwrap_or(f64::NAN);
            if v.is_finite() {
                out.push_str(&crate::value::number_to_string(v));
            } else {
                out.push_str("null");
            }
        }
        out.push_str("}}");
        out
    }

    fn write_document_json(&self, doc: &Document, out: &mut String) {
        match doc {
            Document::Raw(raw) => raw.value().write_json(out),
            Document::Built(built) => {
                out.push('{');
                let mut first = true;
                for slot in built.ordered_slots(&self.interner) {
                    let mut piece = String::new();
                    let name = match slot {
                        Slot::Key => {
                            if !built.key().write_json(&mut piece) {
                                // `JSON.stringify` drops undefined properties.
                                continue;
                            }
                            KEY_PROPERTY
                        }
                        Slot::Term(i) => {
                            let (id, count) = built.entries()[i as usize];
                            piece.push_str(&crate::value::number_to_string(count));
                            self.interner.name(id)
                        }
                    };
                    if !first {
                        out.push(',');
                    }
                    first = false;
                    write_json_string(name, out);
                    out.push(':');
                    out.push_str(&piece);
                }
                out.push('}');
            }
        }
    }

    // -- internals ----------------------------------------------------------

    /// `buildDocument(text, key)`.
    fn build_document(&mut self, input: DocumentInput<'_>, key: DocKey) -> Document {
        match input {
            DocumentInput::Raw(value) => Document::Raw(RawDocument::new(value)),
            DocumentInput::Tokens(tokens) => {
                let mut doc = BuiltDocument::new(key);
                for term in tokens {
                    let id = self.interner.intern(term);
                    // `stopOut` stays undefined for an array, and `!undefined`
                    // is true, so nothing is filtered.
                    doc.observe(id, term, false, &self.interner);
                }
                Document::Built(doc)
            }
            DocumentInput::Text(text) => {
                // With the default tokenizer the whole per-token pipeline —
                // lowercase, tokenize, stop-word test, intern, `observe` — is
                // replaced by `fast_build`'s fused scan, which is
                // differentially verified against the loop below to produce
                // byte-exact state (same ids, entries, counts, and keys).
                if globals::tokenizer_is_default() {
                    return Document::Built(crate::fast_build::build_text(
                        &mut self.interner,
                        &mut self.scratch,
                        text,
                        key,
                    ));
                }
                let lowered = lowercase_units(text);
                let mut doc = BuiltDocument::new(key);
                let interner = &mut self.interner;
                globals::tokenize_global(&lowered).for_each(|term| {
                    let filtered = globals::is_stopword(term);
                    let id = interner.intern(term);
                    doc.observe(id, term, filtered, interner);
                });
                Document::Built(doc)
            }
        }
    }

    /// `this.documents.push(document)`, keeping the derived indexes in step.
    fn push(&mut self, document: Document, reading: &str) -> Result<(), TfIdfError> {
        let documents = self
            .documents
            .as_mut()
            .ok_or_else(|| TfIdfError::UndefinedRead(reading.to_owned()))?;
        match &document {
            Document::Built(doc) => {
                for (id, count) in doc.entries() {
                    if *count > 0.0 {
                        *self.built_df.entry(*id).or_insert(0) += 1;
                    }
                }
            }
            Document::Raw(_) => self.exotic.push(documents.len()),
        }
        documents.push(document);
        Ok(())
    }

    fn rebuild_exotic(&mut self) {
        self.exotic.clear();
        if let Some(documents) = &self.documents {
            for (i, doc) in documents.iter().enumerate() {
                if matches!(doc, Document::Raw(_)) {
                    self.exotic.push(i);
                }
            }
        }
    }

    /// `for (const term in this._idfCache) this.idf(term, true)`.
    fn restore_cache(&mut self) -> Result<(), TfIdfError> {
        for term in self.cache.ordered_keys() {
            self.idf_value(&term, true)?;
        }
        Ok(())
    }

    /// `if (!_.isArray(terms)) terms = tokenizer.tokenize(terms.toString().toLowerCase())`.
    fn resolve(terms: Terms<'_>) -> Resolved<'_> {
        match terms {
            Terms::Tokens(t) => Resolved::Borrowed(t),
            Terms::Text(text) => {
                let lowered = lowercase_units(text);
                let mut out = Vec::new();
                globals::tokenize_global(&lowered).for_each(|t| out.push(t.to_owned()));
                Resolved::Owned(out)
            }
        }
    }
}

/// `String.prototype.toLowerCase`, borrowing when it would be the identity.
///
/// `str::to_lowercase` allocates unconditionally, and a corpus is usually
/// already lowercase ASCII — headlines and mixed-case prose are the exception,
/// not the rule. One pass looking for a byte that could possibly change is
/// cheaper than one allocation plus a full rewrite.
///
/// The predicate is deliberately conservative: any non-ASCII byte hands the
/// work to `to_lowercase`, so the Unicode special cases that matter here
/// ('İ' expanding to two code points, final sigma) are untouched. Only pure
/// ASCII with no uppercase letter takes the borrowing path, where lowercasing
/// provably changes nothing.
fn lowercase_units(text: &str) -> std::borrow::Cow<'_, str> {
    if text.bytes().any(|b| b >= 0x80 || b.is_ascii_uppercase()) {
        std::borrow::Cow::Owned(text.to_lowercase())
    } else {
        std::borrow::Cow::Borrowed(text)
    }
}

/// Maps a failed property read onto the `TypeError` the reference would throw.
fn read_error(target: ReadTarget, term: &str) -> TfIdfError {
    match target {
        ReadTarget::Null => TfIdfError::NullRead(term.to_owned()),
        ReadTarget::Undefined => TfIdfError::UndefinedRead(term.to_owned()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::Slot;
    use crate::globals::{StopwordElement, StopwordList};
    use std::sync::Mutex;

    /// Serialises every test in this module.
    ///
    /// Only two of them *set* a global, but the tokenizer and the stop-word
    /// list are shared by the whole process, so an unguarded test can observe
    /// another one's tokenizer mid-flight. That is not a test-harness detail —
    /// it is the behaviour under test, which is precisely why it has to be
    /// serialised rather than designed away.
    static GLOBALS: Mutex<()> = Mutex::new(());

    fn lock() -> std::sync::MutexGuard<'static, ()> {
        GLOBALS
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn corpus(texts: &[&str]) -> TfIdf {
        let mut t = TfIdf::new();
        for (i, text) in texts.iter().enumerate() {
            #[expect(clippy::cast_precision_loss, reason = "test corpora are tiny")]
            t.add_document(DocumentInput::Text(text), DocKey::Num(i as f64), false)
                .expect("a fresh instance always has a documents array");
        }
        t
    }

    /// The terms of document `d`, in `for…in` order.
    fn keys(t: &TfIdf, d: usize) -> Vec<String> {
        let Some(Document::Built(doc)) = t.documents().and_then(|v| v.get(d)) else {
            panic!("document {d} is not a built map")
        };
        doc.ordered_slots(t.interner())
            .into_iter()
            .filter_map(|slot| match slot {
                Slot::Key => None,
                Slot::Term(i) => Some(t.interner().name(doc.entries()[i as usize].0).to_owned()),
            })
            .collect()
    }

    // -- input classes ------------------------------------------------------

    #[test]
    fn empty_input() {
        let _guard = lock();
        let mut t = corpus(&[""]);
        assert!(keys(&t, 0).is_empty());
        assert_eq!(t.to_json(), r#"{"documents":[{"__key":0}],"_idfCache":{}}"#);
        assert_eq!(t.tfidf(Terms::Text(""), 0).unwrap(), 0.0);
        assert!(t.list_terms(0).unwrap().is_empty());
    }

    #[test]
    fn single_character() {
        let _guard = lock();
        // Every single ASCII letter and digit is in the default stop-word list,
        // so a one-character string document has no terms at all — which is
        // also why so many expectations below are shorter than they look.
        let t = corpus(&["a", "q", "1"]);
        for d in 0..3 {
            assert!(keys(&t, d).is_empty(), "document {d}");
        }
    }

    #[test]
    fn uppercase_is_folded_for_string_documents_only() {
        let _guard = lock();
        let t = corpus(&["Node NODE node"]);
        assert_eq!(keys(&t, 0), ["node"]);

        let mut arrays = TfIdf::new();
        arrays
            .add_document(
                DocumentInput::Tokens(&["Node", "NODE", "node"]),
                DocKey::Undefined,
                false,
            )
            .unwrap();
        assert_eq!(keys(&arrays, 0), ["Node", "NODE", "node"]);
    }

    #[test]
    fn accented_latin_is_split_by_the_word_class() {
        let _guard = lock();
        // `[A-Za-zА-Яа-я0-9_]` excludes every accented character, so `café`
        // tokenizes to `caf` and `naïve` to `na` + `ve`.
        // (`me` is a stop word and `l`/`e` are single letters, hence absent.)
        let t = corpus(&["naïve café crème brûlée"]);
        assert_eq!(keys(&t, 0), ["na", "ve", "caf", "cr", "br"]);
    }

    #[test]
    fn turkish_dotted_i_expands_before_tokenizing() {
        let _guard = lock();
        // `'İ'.toLowerCase()` is 'i' + U+0307, and the combining dot is not a
        // word character — so one word becomes two tokens, of which the bare
        // `i` is then stop-worded away.
        let t = corpus(&["İstanbul"]);
        assert_eq!(keys(&t, 0), ["stanbul"]);
    }

    #[test]
    fn greek_final_sigma_and_cyrillic() {
        let _guard = lock();
        // Greek is outside the word class entirely; Cyrillic А-Я/а-я is inside,
        // but Ё/ё are not.
        let t = corpus(&["ΑΣ ΟΣ", "Москва и Ленинград", "ёлка ель"]);
        assert!(keys(&t, 0).is_empty());
        assert_eq!(keys(&t, 1), ["москва", "и", "ленинград"]);
        assert_eq!(keys(&t, 2), ["лка", "ель"]);
    }

    #[test]
    fn cjk_is_dropped_entirely() {
        let _guard = lock();
        let t = corpus(&["日本語 test 中文测试"]);
        assert_eq!(keys(&t, 0), ["test"]);
    }

    #[test]
    fn astral_characters_separate_words() {
        let _guard = lock();
        let t = corpus(&["😀abc😀", "a😀b", "𝕳𝖊𝖑𝖑𝖔"]);
        assert_eq!(keys(&t, 0), ["abc"]);
        // `a` and `b` survive the split but not the stop-word list.
        assert!(keys(&t, 1).is_empty());
        // Mathematical bold capitals are outside the word class entirely.
        assert!(keys(&t, 2).is_empty());
    }

    #[test]
    fn punctuation_separates_and_underscore_does_not() {
        let _guard = lock();
        let t = corpus(&["e.g. U.S.A. don't hyphen-ated_word"]);
        assert_eq!(keys(&t, 0), ["don", "hyphen", "ated_word"]);
    }

    #[test]
    fn digit_runs_are_terms_and_hoist_to_the_front() {
        let _guard = lock();
        let t = corpus(&["zeta 2020 alpha 10 beta 3.14"]);
        // `3` is a stop word; `10`, `14` and `2020` hoist ahead of the words.
        assert_eq!(keys(&t, 0), ["10", "14", "2020", "zeta", "alpha", "beta"]);
    }

    #[test]
    fn very_long_input() {
        let _guard = lock();
        let text = "lorem ipsum ".repeat(20_000);
        let mut t = corpus(&[&text]);
        assert_eq!(keys(&t, 0), ["lorem", "ipsum"]);
        assert_eq!(t.tf_at("lorem", 0).unwrap().to_number(), 20_000.0);
        let long_word = "a".repeat(100_000);
        t.add_document(DocumentInput::Text(&long_word), DocKey::Undefined, false)
            .unwrap();
        assert_eq!(keys(&t, 1), [long_word]);
    }

    // -- behaviour ----------------------------------------------------------

    #[test]
    fn readme_calculations() {
        let _guard = lock();
        let mut t = corpus(&[
            "this document is about node.",
            "this document is about ruby.",
            "this document is about ruby and node.",
            "this document is about node. it has node examples",
        ]);
        assert_eq!(t.idf("node").unwrap(), 1.0 + crate::math_log(4.0 / 4.0));
        assert_eq!(t.tfidfs(Terms::Text("node")).unwrap(), [1.0, 0.0, 1.0, 2.0]);
        assert_eq!(
            t.tfidf(Terms::Text("ruby"), 3).unwrap(),
            0.0,
            "a term absent from the document contributes nothing"
        );
    }

    #[test]
    fn removing_a_document_updates_the_document_frequency() {
        let _guard = lock();
        let mut t = TfIdf::new();
        for key in ["a", "b"] {
            t.add_document(
                DocumentInput::Text("shared term"),
                DocKey::string(key),
                false,
            )
            .unwrap();
        }
        assert_eq!(t.idf("shared").unwrap(), 1.0 + crate::math_log(2.0 / 3.0));
        assert!(t.remove_document(&DocKey::string("a")).unwrap());
        assert_eq!(t.idf("shared").unwrap(), 1.0 + crate::math_log(1.0 / 2.0));
        assert!(!t.remove_document(&DocKey::string("a")).unwrap());
    }

    #[test]
    fn an_empty_corpus_yields_negative_infinity() {
        let _guard = lock();
        let mut t = TfIdf::new();
        assert_eq!(t.idf("anything").unwrap(), f64::NEG_INFINITY);
        // …and -Infinity is truthy, so it is cached and serialized as null.
        assert_eq!(
            t.to_json(),
            r#"{"documents":[],"_idfCache":{"anything":null}}"#
        );
    }

    #[test]
    fn out_of_range_documents_throw_only_once_there_is_a_term() {
        let _guard = lock();
        let mut t = corpus(&["alpha"]);
        assert_eq!(t.tfidf(Terms::Tokens(&[]), 99).unwrap(), 0.0);
        let err = t.tfidf(Terms::Text("alpha"), 99).unwrap_err();
        assert_eq!(
            err.to_string(),
            "Cannot read properties of undefined (reading 'alpha')"
        );
        assert!(t.list_terms(99).unwrap().is_empty());
    }

    #[test]
    fn round_trips_through_json() {
        let _guard = lock();
        let mut t = TfIdf::new();
        t.add_document(
            DocumentInput::Text("document one"),
            DocKey::string("un"),
            false,
        )
        .unwrap();
        t.add_document(
            DocumentInput::Text("document Two"),
            DocKey::string("deux"),
            false,
        )
        .unwrap();
        let json = t.to_json();
        assert_eq!(
            json,
            r#"{"documents":[{"__key":"un","document":1,"one":1},{"__key":"deux","document":1,"two":1}],"_idfCache":{}}"#
        );

        let mut back = TfIdf::from_json(&json).expect("valid JSON");
        assert_eq!(back.tfidf(Terms::Text("one"), 0).unwrap(), 1.0);
        assert_eq!(back.tfidf(Terms::Text("two"), 0).unwrap(), 0.0);
    }

    #[test]
    fn a_deserialized_object_without_documents_throws_everywhere() {
        let _guard = lock();
        let mut t = TfIdf::from_value(&JsonValue::Obj(Vec::new()));
        assert_eq!(
            t.idf("node").unwrap_err().to_string(),
            "Cannot read properties of undefined (reading 'reduce')"
        );
        assert_eq!(
            t.tfidfs(Terms::Text("node")).unwrap_err().to_string(),
            "Cannot read properties of undefined (reading 'length')"
        );
        assert_eq!(
            t.remove_document(&DocKey::Undefined)
                .unwrap_err()
                .to_string(),
            "Cannot read properties of undefined (reading 'findIndex')"
        );
    }

    #[test]
    fn falsy_constructor_arguments_take_the_empty_branch() {
        let _guard = lock();
        for falsy in [
            JsonValue::Num(0.0),
            JsonValue::Str(String::new()),
            JsonValue::Null,
            JsonValue::Bool(false),
            JsonValue::Num(f64::NAN),
        ] {
            assert_eq!(
                TfIdf::from_value(&falsy).documents().map(<[_]>::len),
                Some(0)
            );
        }
    }

    #[test]
    fn raw_documents_occupy_a_slot_without_matching_anything() {
        let _guard = lock();
        let mut t = corpus(&["node", "node"]);
        t.add_document(
            DocumentInput::Raw(JsonValue::Obj(vec![(
                "text".into(),
                JsonValue::Str("node".into()),
            )])),
            DocKey::string("K"),
            false,
        )
        .unwrap();
        // Three documents, two of which contain the term.
        assert_eq!(t.idf("node").unwrap(), 1.0 + crate::math_log(3.0 / 3.0));
        // The raw object never received a __key, so `undefined` removes it.
        assert!(!t.remove_document(&DocKey::string("K")).unwrap());
        assert!(t.remove_document(&DocKey::Undefined).unwrap());
    }

    #[test]
    fn invalid_encodings_are_rejected_before_the_read() {
        let _guard = lock();
        let mut t = TfIdf::new();
        let err = t
            .add_file_sync("/nonexistent", Some("foobar"), DocKey::Undefined, false)
            .unwrap_err();
        assert_eq!(err.to_string(), "Invalid encoding: foobar");
        // 'raw' is in the module's dead legacy switch but not in Buffer.
        assert!(matches!(
            t.add_file_sync("/nonexistent", Some("raw"), DocKey::Undefined, false),
            Err(TfIdfError::InvalidEncoding(_))
        ));
    }

    // -- process-global state ------------------------------------------------

    #[test]
    fn forcing_past_a_prototype_backed_cache_still_cannot_store_proto() {
        let _guard = lock();
        let mut t = TfIdf::from_value(&JsonValue::Obj(vec![(
            "documents".into(),
            JsonValue::Arr(vec![JsonValue::Obj(vec![(
                "a".into(),
                JsonValue::Num(1.0),
            )])]),
        )]));
        // The truthiness probe finds Object.prototype and returns it.
        assert!(matches!(
            t.idf_value("__proto__", false).unwrap(),
            DynValue::Prototype(Proto::Object)
        ));
        // `force` recomputes — but writing the result back invokes the
        // `__proto__` setter, which ignores numbers, so nothing is cached...
        assert!(matches!(
            t.idf_value("__proto__", true).unwrap(),
            DynValue::Num(_)
        ));
        assert!(t.idf_cache().iter().all(|(k, _)| k != "__proto__"));
        // …and the next unforced call finds the prototype again.
        assert!(matches!(
            t.idf_value("__proto__", false).unwrap(),
            DynValue::Prototype(Proto::Object)
        ));

        // On a null-prototype cache — the one addDocument installs — the same
        // key behaves like any other.
        t.add_document(DocumentInput::Text("alpha"), DocKey::Undefined, false)
            .unwrap();
        assert!(matches!(
            t.idf_value("__proto__", false).unwrap(),
            DynValue::Num(_)
        ));
        assert!(t.idf_cache().iter().any(|(k, _)| k == "__proto__"));
    }

    #[test]
    fn add_file_sync_leaves_a_prototype_backed_cache() {
        let _guard = lock();
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .ancestors()
            .nth(2)
            .expect("crate is two levels below the workspace root")
            .join("benches/data/corpus/tfidf");
        let Ok(()) = std::fs::metadata(&root).map(|_| ()) else {
            return; // optional corpus not present in this checkout
        };

        let mut t = TfIdf::new();
        t.add_file_sync(root.join("one"), None, DocKey::string("one"), false)
            .unwrap();
        // `addFileSync` restores `{}`, not `Object.create(null)`.
        assert!(t.idf_cache_is_prototype_backed());
        assert!(matches!(
            t.idf_value("toString", false).unwrap(),
            DynValue::Function("toString")
        ));
        assert!(t.tfidf(Terms::Tokens(&["toString"]), 0).unwrap().is_nan());

        // base64 decodes the bytes into base64 *text*, which is then tokenized.
        let mut b64 = TfIdf::new();
        b64.add_file_sync(root.join("one"), Some("base64"), DocKey::Undefined, false)
            .unwrap();
        // Node: Buffer.from(fs.readFileSync(path)).toString('base64') is
        // "ZGlzayBkb2N1bWVudCBvbmU=", lowercased and stripped of the padding.
        assert_eq!(keys(&b64, 0), ["zglzaybkb2n1bwvudcbvbmu"]);

        // …and one document later, addDocument swaps the cache back.
        t.add_document(DocumentInput::Text("x"), DocKey::Undefined, false)
            .unwrap();
        assert!(!t.idf_cache_is_prototype_backed());
    }

    #[test]
    fn serializing_an_instance_without_documents_omits_the_key() {
        let _guard = lock();
        let t = TfIdf::from_value(&JsonValue::Obj(Vec::new()));
        assert_eq!(t.to_json(), r#"{"_idfCache":{}}"#);
    }

    #[test]
    fn is_send_and_sync() {
        const fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<TfIdf>();
        assert_send_sync::<Document>();
        assert_send_sync::<DocKey>();
        assert_send_sync::<DynValue>();
    }

    #[test]
    fn set_tokenizer_affects_instances_created_earlier() {
        let _guard = lock();
        let mut a = TfIdf::new();
        a.add_document(
            DocumentInput::Text("this isn't node"),
            DocKey::string("a"),
            false,
        )
        .unwrap();
        assert_eq!(keys(&a, 0), ["isn", "node"]);

        // Installed through a *different* instance, as the reference spec does.
        TfIdf::set_tokenizer(Arc::new(verbora_tokenizers::TreebankWordTokenizer::new()));
        a.add_document(
            DocumentInput::Text("this isn't node"),
            DocKey::string("b"),
            false,
        )
        .unwrap();
        assert_eq!(keys(&a, 1), ["n't", "node"]);
        globals::reset_tokenizer();
    }

    #[test]
    fn stopword_validation_matches_the_reference() {
        let _guard = lock();
        assert!(TfIdf::set_stopwords(&StopwordList::of(["node"])));
        assert!(TfIdf::set_stopwords(
            &StopwordList::of(Vec::<String>::new())
        ));
        assert!(!TfIdf::set_stopwords(&StopwordList::NotAnArray));
        assert!(!TfIdf::set_stopwords(&StopwordList::Array(vec![
            StopwordElement::Str("ok".into()),
            StopwordElement::NotAString,
        ])));

        // The rejected calls left the empty list installed, so nothing filters.
        let t = corpus(&["this document is about node."]);
        assert_eq!(keys(&t, 0), ["this", "document", "is", "about", "node"]);
        globals::reset_stopwords();
    }

    #[test]
    fn the_shared_stopword_array_is_aliased() {
        let _guard = lock();
        globals::reset_stopwords();
        verbora_core::stopwords::add_global_stopword("mango");
        let t = corpus(&["node and mango"]);
        assert_eq!(keys(&t, 0), ["node"]);
        verbora_core::stopwords::remove_global_stopword("mango");
        let t = corpus(&["node and mango"]);
        assert_eq!(keys(&t, 0), ["node", "mango"]);
    }

    // -- fast-path differential parity ---------------------------------------
    //
    // `build_document`'s `Text` branch dispatches to `fast_build` whenever the
    // process-global tokenizer is the default one, so the tests below hold it
    // to the reference per-token loop — the pre-fast-path `add_document` body,
    // reproduced verbatim in `reference_add_document` — on the full observable
    // state: term-id assignment order, per-document entries (ids, counts,
    // insertion order), document keys, the incremental document-frequency
    // table, and the serialized JSON.

    /// The reference `Text` ingestion loop: what `add_document` executed
    /// before `fast_build` existed, kept as the parity oracle.
    fn reference_add_document(t: &mut TfIdf, text: &str, key: DocKey) {
        let lowered = lowercase_units(text);
        let mut doc = BuiltDocument::new(key);
        let interner = &mut t.interner;
        globals::tokenize_global(&lowered).for_each(|term| {
            let filtered = globals::is_stopword(term);
            let id = interner.intern(term);
            doc.observe(id, term, filtered, interner);
        });
        t.push(Document::Built(doc), "push")
            .expect("fresh instance");
        t.cache = IdfCache::null_prototype();
    }

    /// One document's state: its key (via `Debug`) and its entries in
    /// insertion order, with ids kept so id-assignment order is compared too.
    type DocState = (String, Vec<(TermId, String, f64)>);

    /// Everything observable about a corpus's built state, id-exact.
    fn full_state(t: &TfIdf) -> (Vec<DocState>, Vec<(TermId, u32)>) {
        let docs = t
            .documents()
            .expect("built corpora have documents")
            .iter()
            .map(|d| match d {
                Document::Built(b) => (
                    format!("{:?}", b.key()),
                    b.entries()
                        .iter()
                        .map(|(id, c)| (*id, t.interner().name(*id).to_owned(), *c))
                        .collect(),
                ),
                Document::Raw(_) => panic!("differential corpora contain no raw documents"),
            })
            .collect();
        let mut df: Vec<(TermId, u32)> = t.built_df.iter().map(|(k, v)| (*k, *v)).collect();
        df.sort_unstable();
        (docs, df)
    }

    fn assert_fast_matches_reference(inputs: &[(String, DocKey)], label: &str) {
        let mut fast = TfIdf::new();
        for (text, key) in inputs {
            fast.add_document(DocumentInput::Text(text), key.clone(), false)
                .expect("fresh instance");
        }
        let mut reference = TfIdf::new();
        for (text, key) in inputs {
            reference_add_document(&mut reference, text, key.clone());
        }
        assert_eq!(
            full_state(&fast),
            full_state(&reference),
            "{label}: state diverged"
        );
        assert_eq!(
            fast.to_json(),
            reference.to_json(),
            "{label}: JSON diverged"
        );
    }

    /// The competitive bench's fallback corpus (scaled down), whose rotation
    /// varies token boundaries against the 64-byte bitmap words.
    #[test]
    fn fast_build_matches_the_reference_loop_on_the_bench_corpus() {
        let _guard = lock();
        let text =
            "the quick brown fox jumps over the lazy dog while node and ruby argue ".repeat(240);
        let words: Vec<&str> = text.split_whitespace().collect();
        for n in [4usize, 16, 64] {
            let inputs: Vec<(String, DocKey)> = (0..n)
                .map(|i| {
                    let start = (i * 7) % words.len();
                    #[expect(clippy::cast_precision_loss, reason = "test corpora are tiny")]
                    let key = DocKey::Num(i as f64);
                    (words[start..].join(" "), key)
                })
                .collect();
            assert_fast_matches_reference(&inputs, &format!("bench corpus n={n}"));
        }
    }

    /// Randomized corpora over an adversarial vocabulary: `__proto__`/`__key`
    /// collisions, `Object.prototype` method names, digit runs, every
    /// lowercasing special case (dotted İ, final sigma, ẞ), non-word scripts,
    /// astral characters, and mixed case — each exercising a different
    /// dispatch tier of the fast path (borrowed ASCII, lowered ASCII,
    /// char-exact fallback).
    #[test]
    fn fast_build_matches_the_reference_loop_on_adversarial_corpora() {
        let _guard = lock();
        const VOCAB: &[&str] = &[
            "the",
            "quick",
            "node",
            "ruby",
            "__proto__",
            "__key",
            "toString",
            "constructor",
            "hasOwnProperty",
            "valueOf",
            "2020",
            "10",
            "0",
            "4294967295",
            "İstanbul",
            "naïve",
            "ΑΣ",
            "ΟΣ",
            "Σ",
            "ёлка",
            "Москва",
            "don't",
            "a",
            "_",
            "___",
            "e.g.",
            "😀abc😀",
            "𝕳𝖊𝖑𝖑𝖔",
            "MiXeD",
            "CASE",
            "ẞ",
            "ǅungla",
            "over",
            "and",
            "while",
            "length",
            "日本語",
            "exactly8",
            "morethan8bytes",
        ];
        let mut state = 0x243F_6A88_85A3_08D3u64;
        let mut xorshift = move || {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state
        };
        for case in 0..4000u32 {
            let ndocs = 1 + (xorshift() % 5) as usize;
            let inputs: Vec<(String, DocKey)> = (0..ndocs)
                .map(|i| {
                    let ntokens = (xorshift() % 40) as usize;
                    let mut text = String::new();
                    for k in 0..ntokens {
                        if k > 0 {
                            match xorshift() % 4 {
                                0 => text.push(' '),
                                1 => text.push_str("  "),
                                2 => text.push('.'),
                                _ => text.push_str(", "),
                            }
                        }
                        text.push_str(VOCAB[(xorshift() % VOCAB.len() as u64) as usize]);
                    }
                    #[expect(clippy::cast_precision_loss, reason = "test corpora are tiny")]
                    let key = match xorshift() % 6 {
                        0 => DocKey::Undefined,
                        1 => DocKey::Null,
                        2 => DocKey::Num(i as f64),
                        3 => DocKey::Num(0.0),
                        4 => DocKey::string(format!("k{i}")),
                        _ => DocKey::Bool(i % 2 == 0),
                    };
                    (text, key)
                })
                .collect();
            assert_fast_matches_reference(&inputs, &format!("case {case}"));
        }
    }

    #[test]
    fn key_and_proto_tokens_through_the_text_path() {
        let _guard = lock();
        // `__key` corrupts a string key by concatenation; `__proto__` is
        // silently dropped — both through the fast `Text` path, not `observe`
        // directly.
        let mut t = TfIdf::new();
        t.add_document(
            DocumentInput::Text("__key __key alpha __proto__"),
            DocKey::string("mykey"),
            false,
        )
        .unwrap();
        assert_eq!(
            t.to_json(),
            r#"{"documents":[{"__key":"mykey11","alpha":1}],"_idfCache":{}}"#
        );
    }

    #[test]
    fn prototype_method_zero_slot_through_the_text_path() {
        let _guard = lock();
        // `constructor` is the one `Object.prototype` member that survives
        // lowercasing, so it is the only way the `Text` path can reach the
        // issue-#119 zero-before-count rule.
        let t = corpus(&["constructor constructor alpha"]);
        assert_eq!(
            t.to_json(),
            r#"{"documents":[{"__key":0,"constructor":2,"alpha":1}],"_idfCache":{}}"#
        );

        // Stop-worded, the zeroed slot must survive as an own `0` property.
        assert!(TfIdf::set_stopwords(&StopwordList::of(["constructor"])));
        let t = corpus(&["constructor alpha"]);
        globals::reset_stopwords();
        assert_eq!(
            t.to_json(),
            r#"{"documents":[{"__key":0,"constructor":0,"alpha":1}],"_idfCache":{}}"#
        );
    }

    #[test]
    fn a_replaced_stopword_list_reaches_the_fast_path() {
        let _guard = lock();
        // With the default list replaced, default stop words like `and` must
        // come back and only the custom word may be filtered — resolved per
        // document, so the swap takes effect with no explicit invalidation.
        assert!(TfIdf::set_stopwords(&StopwordList::of(["node"])));
        let t = corpus(&["node and ruby"]);
        globals::reset_stopwords();
        assert_eq!(keys(&t, 0), ["and", "ruby"]);

        let t = corpus(&["node and ruby"]);
        assert_eq!(keys(&t, 0), ["node", "ruby"]);
    }
}
