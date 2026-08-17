//! Word → part-of-speech dictionaries.
//!
//! # The shared-mutation contract
//!
//! `new Lexicon(lang, ...)` does
//!
//! ```text
//! this.lexicon = Object.assign(sharedJsonObject, extendedLexicon || {})
//! ```
//!
//! which mutates the module-level JSON object *and* hands out the same object.
//! Every `Lexicon` for a language in a process therefore shares one mutable
//! dictionary, and `addWord`, the `extendedLexicon` constructor argument and
//! [`Corpus::build_lexicon`](crate::Corpus::build_lexicon) all write into it
//! permanently — visible even to instances created **before** the write.
//!
//! That is reproduced here, because programs that build a lexicon from a corpus
//! and then tag with a different instance depend on it. The mechanism is a
//! process-global overlay per language, guarded by an `AtomicBool`: a dictionary
//! that has never been written to is read straight out of the executable with no
//! lock, no hash lookup and no allocation, so the common case pays nothing for a
//! quirk it does not use.
//!
//! Two escape hatches exist for callers who want determinism instead:
//! [`Lexicon::detached`] gives an instance-private copy-on-write dictionary, and
//! [`Lexicon::reset_shared`] discards a language's accumulated mutations.
//!
//! # Shipping the data
//!
//! The dictionaries are embedded in the binary as a packed index (see
//! [`crate::data`]) rather than parsed from JSON at start-up:
//!
//! | | JSON at first use | packed index |
//! |---|---|---|
//! | English lexicon ready | ~55 ms | 0 (no initialisation step) |
//! | Bytes in the binary | 4.6 MB | 2.4 MB |
//! | First lookup | after full parse | ~17 byte-compare probes |
//!
//! `benches/tagger.rs` measures the first-lookup cost directly; it is
//! indistinguishable from a warm lookup, which is the point.

use std::borrow::Cow;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{OnceLock, RwLock};

use rustc_hash::FxHashMap;

use crate::data::StaticLexicon;
use crate::error::TaggerError;
use crate::ruleset::Language;
use crate::sentence::Tag;
use crate::utf16;

/// What `Lexicon.tagWord` returned.
///
/// Not a plain `Vec<String>`, because three of its outcomes are distinguishable
/// and the difference reaches the tagged output.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Categories {
    /// The word was in the dictionary. **May be empty**: the English lexicon
    /// contains the key `""` mapped to `[]`, and an empty array is truthy in
    /// the reference, so it is returned as-is rather than falling through to the
    /// default category. The token then ends up with no tag at all.
    Found(Vec<Tag>),
    /// The word was absent, so `[tagWordWithDefaults(word)]` was returned — a
    /// one-element array whose element is `undefined` when no default category
    /// is set.
    Default(Option<Tag>),
    /// The word was `"__proto__"`, whose lookup walks into `Object.prototype`.
    ///
    /// That value is truthy and is not a function, so neither of `tagWord`'s two
    /// guards rejects it and it is returned; `categories[0]` is then `undefined`
    /// and the token is left untagged. A Rust `HashMap` has no prototype chain,
    /// so this case exists only to reproduce the reference.
    ObjectPrototype,
}

impl Categories {
    /// `categories[0]` — what the tagger actually uses.
    #[must_use]
    pub fn first(&self) -> Option<&str> {
        match self {
            Self::Found(v) => v.first().map(|t| &**t),
            Self::Default(d) => d.as_deref(),
            Self::ObjectPrototype => None,
        }
    }

    /// The elements as the reference would see them; `None` is `undefined`.
    ///
    /// `ObjectPrototype` has no elements — `Object.keys(Object.prototype)` is
    /// empty — so it yields an empty slice.
    #[must_use]
    pub fn to_vec(&self) -> Vec<Option<&str>> {
        match self {
            Self::Found(v) => v.iter().map(|t| Some(&**t)).collect(),
            Self::Default(d) => vec![d.as_deref()],
            Self::ObjectPrototype => Vec::new(),
        }
    }
}

/// A value handed to [`Lexicon::tag_word_value`].
///
/// `tagWord` indexes a plain object, so the argument is coerced to a property
/// key first — and three of the resulting keys, `"undefined"`, `"null"` and
/// `"length"`, are **real entries** in the English lexicon. The lookup therefore
/// succeeds for `tagWord(undefined)` on an English lexicon and throws on a Dutch
/// one, where the key is absent and the miss path reaches `word.toLowerCase()`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LookupKey<'a> {
    /// An ordinary string.
    Str(&'a str),
    /// `undefined`; property key `"undefined"`.
    Undefined,
    /// `null`; property key `"null"`.
    Null,
    /// Any other non-string value, with its property key already coerced —
    /// `NonString("5")` for the number `5`.
    ///
    /// The coercion is the caller's because `String(number)` is a specification
    /// algorithm in its own right (`1e21` is `"1e+21"`, not 22 digits) and no
    /// part of this crate needs it.
    NonString(&'a str),
}

impl LookupKey<'_> {
    /// The property key the value is coerced to.
    #[must_use]
    pub const fn key(&self) -> &str {
        match self {
            Self::Str(s) | Self::NonString(s) => s,
            Self::Undefined => "undefined",
            Self::Null => "null",
        }
    }

    /// The error a missed lookup produces, because the fallback calls
    /// `word.toLowerCase()` on something that is not a string.
    const fn miss_error(&self) -> TaggerError {
        match self {
            // A string can always be lowercased, so this never fires for `Str`.
            Self::Str(_) | Self::NonString(_) => TaggerError::WordNotAString,
            Self::Undefined => TaggerError::undefined("toLowerCase"),
            Self::Null => TaggerError::ReadOfUndefined {
                object: "null",
                property: "toLowerCase",
            },
        }
    }
}

/// One runtime-added or runtime-overridden entry.
#[derive(Debug, Clone)]
struct Added {
    key: Box<str>,
    tags: Vec<Tag>,
    /// `true` when this shadows a key the packed base already has, in which case
    /// it does not change the key count or the enumeration position.
    shadows_base: bool,
}

/// A dictionary: an immutable packed base plus an ordered overlay.
#[derive(Debug, Clone, Default)]
struct Dictionary {
    base: Option<StaticLexicon>,
    added: Vec<Added>,
    index: FxHashMap<Box<str>, usize>,
    new_keys: usize,
}

/// Result of one raw key lookup.
enum Hit<'d> {
    Owned(&'d [Tag]),
    Static(StaticLexicon, usize),
    /// `Object.prototype`, reached only through the key `"__proto__"`.
    Proto,
    Miss,
}

/// Whether `key` is a reference language array index — the keys the reference engine hoists to the front
/// of `Object.keys` and sorts numerically. Mirrors the same test in `build.rs`.
fn array_index(key: &str) -> Option<u32> {
    if key.is_empty() || key.len() > 10 || !key.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    if key.len() > 1 && key.starts_with('0') {
        return None;
    }
    let n: u64 = key.parse().ok()?;
    (n < u64::from(u32::MAX)).then_some(n as u32)
}

impl Dictionary {
    fn base_only(base: StaticLexicon) -> Self {
        Self {
            base: Some(base),
            ..Self::default()
        }
    }

    fn lookup(&self, key: &str) -> Hit<'_> {
        if let Some(i) = self.index.get(key) {
            return Hit::Owned(&self.added[*i].tags);
        }
        if let Some(b) = self.base {
            if let Some(i) = b.find(key) {
                return Hit::Static(b, i);
            }
        }
        // No own property: the plain object still inherits from Object.prototype,
        // and `__proto__` is the one inherited name whose value is not a function.
        if key == "__proto__" {
            return Hit::Proto;
        }
        Hit::Miss
    }

    fn first_tag(&self, key: &str) -> Option<Option<Tag>> {
        match self.lookup(key) {
            Hit::Owned(t) => Some(t.first().cloned()),
            Hit::Static(b, i) => Some(b.tags(i).next().map(Cow::Borrowed)),
            Hit::Proto => Some(None),
            Hit::Miss => None,
        }
    }

    fn set(&mut self, key: &str, tags: Vec<Tag>) {
        // `obj['__proto__'] = value` runs the inherited setter instead of
        // creating an own property, so the key count does not change. The
        // prototype swap it performs is not reproduced; see the crate docs.
        if key == "__proto__" {
            return;
        }
        if let Some(i) = self.index.get(key) {
            self.added[*i].tags = tags;
            return;
        }
        let shadows_base = self.base.is_some_and(|b| b.find(key).is_some());
        if !shadows_base {
            self.new_keys += 1;
        }
        self.index.insert(key.into(), self.added.len());
        self.added.push(Added {
            key: key.into(),
            tags,
            shadows_base,
        });
    }

    fn len(&self) -> usize {
        self.base.map_or(0, StaticLexicon::len) + self.new_keys
    }

    /// Visits every entry in the reference's `Object.keys` order.
    fn for_each(&self, f: &mut impl FnMut(&str, &[&str])) {
        let mut buf: Vec<&str> = Vec::new();
        let base_numeric = self.base.map_or(0, StaticLexicon::numeric_prefix_len);

        // Runtime-added keys, partitioned the way the reference engine partitions them.
        let mut added_numeric: Vec<(u32, usize)> = Vec::new();
        let mut added_plain: Vec<usize> = Vec::new();
        for (i, a) in self.added.iter().enumerate() {
            if a.shadows_base {
                continue; // keeps the base's position, not a new one
            }
            match array_index(&a.key) {
                Some(n) => added_numeric.push((n, i)),
                None => added_plain.push(i),
            }
        }
        added_numeric.sort_unstable();

        let emit = |key: &str, tags: &[&str], f: &mut dyn FnMut(&str, &[&str])| f(key, tags);

        // 1. Array-index keys, ascending: the base's hoisted prefix merged with
        //    any added ones. Both sequences are already sorted.
        let mut bi = 0usize;
        let mut ai = 0usize;
        while bi < base_numeric || ai < added_numeric.len() {
            let take_base = if bi >= base_numeric {
                false
            } else if ai >= added_numeric.len() {
                true
            } else {
                let b = self.base.expect("base_numeric > 0 implies a base");
                array_index(b.key(bi)).expect("hoisted keys are array indices")
                    < added_numeric[ai].0
            };
            if take_base {
                let b = self.base.expect("base_numeric > 0 implies a base");
                self.collect_tags(b, bi, &mut buf);
                emit(b.key(bi), &buf, f);
                bi += 1;
            } else {
                let a = &self.added[added_numeric[ai].1];
                buf.clear();
                buf.extend(a.tags.iter().map(|t| &**t));
                emit(&a.key, &buf, f);
                ai += 1;
            }
        }

        // 2. The base's non-index keys, in file order.
        if let Some(b) = self.base {
            for i in base_numeric..b.len() {
                self.collect_tags(b, i, &mut buf);
                emit(b.key(i), &buf, f);
            }
        }

        // 3. Added non-index keys, in insertion order.
        for i in added_plain {
            let a = &self.added[i];
            buf.clear();
            buf.extend(a.tags.iter().map(|t| &**t));
            emit(&a.key, &buf, f);
        }
    }

    /// Fills `buf` with the effective tags of base entry `i`, honouring any
    /// overlay entry that shadows it.
    fn collect_tags<'s>(&'s self, b: StaticLexicon, i: usize, buf: &mut Vec<&'s str>) {
        buf.clear();
        let key = b.key(i);
        if let Some(slot) = self.index.get(key) {
            buf.extend(self.added[*slot].tags.iter().map(|t| &**t));
        } else {
            buf.extend(b.tags(i).map(|t| -> &'s str { t }));
        }
    }
}

// ---------------------------------------------------------------------------
// Process-global dictionaries
// ---------------------------------------------------------------------------

static ENGLISH: OnceLock<RwLock<Dictionary>> = OnceLock::new();
static DUTCH: OnceLock<RwLock<Dictionary>> = OnceLock::new();
/// Set the first time a language's shared dictionary is written to. Until then
/// every read skips the lock entirely.
static ENGLISH_DIRTY: AtomicBool = AtomicBool::new(false);
static DUTCH_DIRTY: AtomicBool = AtomicBool::new(false);

fn shared(language: Language) -> &'static RwLock<Dictionary> {
    match language {
        Language::English => {
            ENGLISH.get_or_init(|| RwLock::new(Dictionary::base_only(StaticLexicon::english())))
        }
        Language::Dutch => {
            DUTCH.get_or_init(|| RwLock::new(Dictionary::base_only(StaticLexicon::dutch())))
        }
    }
}

fn dirty_flag(language: Language) -> &'static AtomicBool {
    match language {
        Language::English => &ENGLISH_DIRTY,
        Language::Dutch => &DUTCH_DIRTY,
    }
}

fn base_of(language: Language) -> StaticLexicon {
    match language {
        Language::English => StaticLexicon::english(),
        Language::Dutch => StaticLexicon::dutch(),
    }
}

/// Where a `Lexicon`'s entries live.
#[derive(Debug, Clone)]
enum Store {
    /// Aliases the process-global dictionary, as every the reference `Lexicon` does.
    Shared(Language),
    /// Instance-private, after `parseLexicon` or [`Lexicon::detached`].
    Owned(Dictionary),
}

/// A word → categories dictionary with default categories for unknown words.
#[derive(Debug, Clone)]
pub struct Lexicon {
    store: Store,
    /// Category assigned to an unknown word. `None` is `undefined`, which
    /// produces a token with no tag rather than an error.
    pub default_category: Option<Tag>,
    /// Category assigned to an unknown word starting with an ASCII `A`–`Z`.
    pub default_category_capitalised: Option<Tag>,
}

impl Lexicon {
    /// `new Lexicon(language, defaultCategory, defaultCategoryCapitalised)`.
    ///
    /// # The language switch
    ///
    /// `'EN'` selects English and **everything else selects Dutch**, including
    /// `None` — `Lexicon::new(None, ..)` is a Dutch lexicon. (`RuleSet`'s switch
    /// defaults the other way; see [`Language::for_rule_set`].)
    ///
    /// # The nested default-category guard
    ///
    /// `defaultCategoryCapitalised` is stored **only if `defaultCategory` is
    /// truthy**, because the reference nests the two assignments. So
    /// `Lexicon::new(Some("EN"), None, Some("NNP"))` and
    /// `Lexicon::new(Some("EN"), Some(""), Some("NNP"))` both leave *both*
    /// defaults unset. [`Lexicon::set_default_categories`] has different rules
    /// again.
    #[must_use]
    pub fn new(
        language: Option<&str>,
        default_category: Option<&str>,
        default_category_capitalised: Option<&str>,
    ) -> Self {
        let (dc, dcc) = nested_defaults(default_category, default_category_capitalised);
        Self {
            store: Store::Shared(Language::for_lexicon(language)),
            default_category: dc,
            default_category_capitalised: dcc,
        }
    }

    /// `new Lexicon(language, dc, dcc, extendedLexicon)`.
    ///
    /// The extension is written into the **shared** dictionary for the language,
    /// exactly as `Object.assign` does — every other `Lexicon` for that language,
    /// including ones constructed earlier, sees it from this point on.
    #[must_use]
    pub fn with_extension(
        language: Option<&str>,
        default_category: Option<&str>,
        default_category_capitalised: Option<&str>,
        extended: &[(&str, Vec<Tag>)],
    ) -> Self {
        let lang = Language::for_lexicon(language);
        if !extended.is_empty() {
            let mut g = shared(lang).write().expect("lexicon lock poisoned");
            for (word, tags) in extended {
                g.set(word, tags.clone());
            }
            dirty_flag(lang).store(true, Ordering::Release);
        }
        Self::new(language, default_category, default_category_capitalised)
    }

    /// A lexicon that does **not** alias the process-global dictionary.
    ///
    /// Same starting contents as [`Lexicon::new`], but `add_word` and friends
    /// write to this instance only. This has no the reference equivalent; it exists
    /// so that library users who want reproducible behaviour are not forced to
    /// inherit a global they never asked for.
    #[must_use]
    pub fn detached(
        language: Option<&str>,
        default_category: Option<&str>,
        default_category_capitalised: Option<&str>,
    ) -> Self {
        let (dc, dcc) = nested_defaults(default_category, default_category_capitalised);
        Self {
            store: Store::Owned(Dictionary::base_only(base_of(Language::for_lexicon(
                language,
            )))),
            default_category: dc,
            default_category_capitalised: dcc,
        }
    }

    /// An empty lexicon with no bundled entries at all.
    #[must_use]
    pub fn empty() -> Self {
        Self {
            store: Store::Owned(Dictionary::default()),
            default_category: None,
            default_category_capitalised: None,
        }
    }

    /// Discards every runtime mutation of a language's shared dictionary.
    ///
    /// Intended for tests and for long-running processes that need to undo a
    /// `Corpus::build_lexicon`. No the reference equivalent — there, the pollution
    /// is permanent for the life of the process.
    pub fn reset_shared(language: Language) {
        let mut g = shared(language).write().expect("lexicon lock poisoned");
        *g = Dictionary::base_only(base_of(language));
        dirty_flag(language).store(false, Ordering::Release);
    }

    /// Runs `f` against the effective dictionary.
    ///
    /// The fast path constructs a base-only view without touching the lock; a
    /// `Dictionary` with an empty overlay allocates nothing, so this is free.
    fn with_dict<R>(&self, f: impl FnOnce(&Dictionary) -> R) -> R {
        match &self.store {
            Store::Owned(d) => f(d),
            Store::Shared(lang) => {
                if dirty_flag(*lang).load(Ordering::Acquire) {
                    let g = shared(*lang).read().expect("lexicon lock poisoned");
                    f(&g)
                } else {
                    f(&Dictionary::base_only(base_of(*lang)))
                }
            }
        }
    }

    fn with_dict_mut<R>(&mut self, f: impl FnOnce(&mut Dictionary) -> R) -> R {
        match &mut self.store {
            Store::Owned(d) => f(d),
            Store::Shared(lang) => {
                let lang = *lang;
                let mut g = shared(lang).write().expect("lexicon lock poisoned");
                let r = f(&mut g);
                dirty_flag(lang).store(true, Ordering::Release);
                r
            }
        }
    }

    /// `tagWord(word)`: the categories for a word.
    ///
    /// The lookup is: exact key, then lowercased key, then the default category.
    /// A *present but empty* entry short-circuits the whole chain, because an
    /// empty array is truthy — see [`Categories::Found`].
    #[must_use]
    pub fn tag_word(&self, word: &str) -> Categories {
        self.with_dict(|d| match d.lookup(word) {
            Hit::Owned(t) => Categories::Found(t.to_vec()),
            Hit::Static(b, i) => Categories::Found(b.tags(i).map(Cow::Borrowed).collect()),
            Hit::Proto => Categories::ObjectPrototype,
            Hit::Miss => {
                let lower = lowercase(word);
                match d.lookup(&lower) {
                    Hit::Owned(t) => Categories::Found(t.to_vec()),
                    Hit::Static(b, i) => Categories::Found(b.tags(i).map(Cow::Borrowed).collect()),
                    Hit::Proto => Categories::ObjectPrototype,
                    Hit::Miss => Categories::Default(self.tag_word_with_defaults(word)),
                }
            }
        })
    }

    /// `tagWord(value)` for an argument that is not a `String`.
    ///
    /// Only the *exact* key is tried: the lowercased retry calls
    /// `word.toLowerCase()`, which throws for every non-string. So the lookup
    /// either hits on the coerced key or fails — and which of the two happens
    /// depends on the language, since `"undefined"` and `"null"` are English
    /// entries but not Dutch ones.
    ///
    /// [`LookupKey::Str`] is accepted too and behaves exactly like
    /// [`Lexicon::tag_word`].
    ///
    /// # Errors
    ///
    /// The `TypeError` the lowercased retry raises: `word.toLowerCase is not a
    /// function` for a number, `Cannot read properties of undefined/null
    /// (reading 'toLowerCase')` for those two.
    pub fn tag_word_value(&self, value: LookupKey<'_>) -> Result<Categories, TaggerError> {
        if let LookupKey::Str(s) = value {
            return Ok(self.tag_word(s));
        }
        self.with_dict(|d| match d.lookup(value.key()) {
            Hit::Owned(t) => Ok(Categories::Found(t.to_vec())),
            Hit::Static(b, i) => Ok(Categories::Found(b.tags(i).map(Cow::Borrowed).collect())),
            Hit::Proto => Ok(Categories::ObjectPrototype),
            Hit::Miss => Err(value.miss_error()),
        })
    }

    /// `tagWord(word)[0]` without materialising the array.
    ///
    /// This is the tagging hot path: one binary search over the packed index,
    /// and — for an already-lowercase ASCII word, which nearly all of them are —
    /// no allocation for the lowercased fallback either.
    #[must_use]
    pub fn first_category(&self, word: &str) -> Option<Tag> {
        self.with_dict(|d| {
            if let Some(t) = d.first_tag(word) {
                return t;
            }
            // Only lowercase (and thus allocate) when the word could differ.
            if word.is_ascii() && !word.bytes().any(|b| b.is_ascii_uppercase()) {
                return self.tag_word_with_defaults(word);
            }
            let lower = lowercase(word);
            d.first_tag(&lower)
                .unwrap_or_else(|| self.tag_word_with_defaults(word))
        })
    }

    /// `tagWordWithDefaults(word)`.
    ///
    /// The capitalisation test is `/[A-Z]/` against the **first UTF-16 code
    /// unit**: ASCII only, so `"Ålesund"` and `"Москва"` take the ordinary
    /// default. The capitalised default also has to be *truthy* to be used.
    #[must_use]
    pub fn tag_word_with_defaults(&self, word: &str) -> Option<Tag> {
        if utf16::first_is_ascii_upper(word) {
            if let Some(c) = self
                .default_category_capitalised
                .as_ref()
                .filter(|c| !c.is_empty())
            {
                return Some(c.clone());
            }
        }
        self.default_category.clone()
    }

    /// `addWord(word, categories)` — unconditional replace.
    ///
    /// On a shared lexicon this writes to the **process-global** dictionary for
    /// the language; see the module documentation.
    pub fn add_word(&mut self, word: &str, categories: Vec<Tag>) {
        self.with_dict_mut(|d| d.set(word, categories));
    }

    /// `parseLexicon(data)`: replaces the contents from `word cat1 cat2 …` text.
    ///
    /// The instance **detaches** from the shared dictionary, which is left
    /// untouched. Lines are split on the reference `\s+`, so `U+FEFF` separates
    /// tokens and `U+0085` does not — `split_whitespace` gets both backwards.
    /// A line with a single token stores an *empty* category list, which
    /// [`Lexicon::tag_word`] then returns as-is.
    ///
    /// # Errors
    ///
    /// Input with no non-newline characters (`""`, `"\n\n"`) makes
    /// `String.match` with `/g` return `null`, and the reference then calls
    /// `.forEach` on it. That TypeError is part of the contract.
    pub fn parse_lexicon(&mut self, data: &str) -> Result<(), TaggerError> {
        let lines: Vec<&str> = data.split(['\r', '\n']).filter(|l| !l.is_empty()).collect();
        if lines.is_empty() {
            return Err(TaggerError::ReadOfUndefined {
                object: "null",
                property: "forEach",
            });
        }
        let mut dict = Dictionary::default();
        for line in lines {
            let trimmed = line.trim_matches(verbora_core::whitespace::is_whitespace);
            // `"".split(/\s+/)` is `[""]`, not `[]`, so a blank line stores the
            // key "" with no categories.
            let mut parts = split_whitespace_units(trimmed);
            let key = parts.next().unwrap_or("");
            let tags: Vec<Tag> = parts.map(|t| Cow::Owned(t.to_string())).collect();
            dict.set(key, tags);
        }
        self.store = Store::Owned(dict);
        Ok(())
    }

    /// `setDefaultCategories(category, categoryCapitalised)`.
    ///
    /// Differs from the constructor in both directions: `category` is stored
    /// **unconditionally** (a falsy value really does become the default), while
    /// a falsy `categoryCapitalised` leaves any previous value **intact** rather
    /// than clearing it.
    pub fn set_default_categories(&mut self, category: Option<&str>, capitalised: Option<&str>) {
        self.default_category = category.map(|c| Cow::Owned(c.to_string()));
        if let Some(c) = capitalised.filter(|c| !c.is_empty()) {
            self.default_category_capitalised = Some(Cow::Owned(c.to_string()));
        }
    }

    /// `nrEntries()` / `size()`: the number of own keys.
    #[must_use]
    pub fn len(&self) -> usize {
        self.with_dict(Dictionary::len)
    }

    /// Whether the lexicon has no entries.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// `prettyPrint()`: one tab-separated line per entry.
    ///
    /// Note the **trailing tab** before every newline, and that an entry with no
    /// categories prints as `word\t\n`. Keys come out in the reference enumeration
    /// order: array-index-like keys first in ascending numeric order, then the
    /// rest in insertion order.
    #[must_use]
    pub fn pretty_print(&self) -> String {
        let mut out = String::new();
        self.for_each_entry(&mut |word, tags| {
            out.push_str(word);
            out.push('\t');
            for t in tags {
                out.push_str(t);
                out.push('\t');
            }
            out.push('\n');
        });
        out
    }

    /// Visits every entry in the reference key order.
    pub fn for_each_entry(&self, f: &mut impl FnMut(&str, &[&str])) {
        self.with_dict(|d| d.for_each(f));
    }
}

/// The constructor's nested guard, isolated so both constructors share it.
fn nested_defaults(dc: Option<&str>, dcc: Option<&str>) -> (Option<Tag>, Option<Tag>) {
    match dc.filter(|c| !c.is_empty()) {
        None => (None, None),
        Some(c) => (
            Some(Cow::Owned(c.to_string())),
            dcc.filter(|c| !c.is_empty())
                .map(|c| Cow::Owned(c.to_string())),
        ),
    }
}

/// `String.prototype.toLowerCase`, with a zero-allocation ASCII fast path.
fn lowercase(word: &str) -> Cow<'_, str> {
    if word.is_ascii() {
        if word.bytes().any(|b| b.is_ascii_uppercase()) {
            Cow::Owned(word.to_ascii_lowercase())
        } else {
            Cow::Borrowed(word)
        }
    } else {
        // Full Unicode: Greek final sigma and the Turkish dotted capital must
        // fold the way the reference folds them, so `to_ascii_lowercase` will not do.
        Cow::Owned(word.to_lowercase())
    }
}

/// `str.split(/\s+/)` with the reference's `\s`, preserving the leading empty field
/// that a string starting with whitespace produces.
fn split_whitespace_units(s: &str) -> impl Iterator<Item = &str> {
    let mut rest = Some(s);
    std::iter::from_fn(move || {
        let cur = rest?;
        match cur.find(verbora_core::whitespace::is_whitespace) {
            None => {
                rest = None;
                Some(cur)
            }
            Some(at) => {
                let (head, tail) = cur.split_at(at);
                let tail = tail.trim_start_matches(verbora_core::whitespace::is_whitespace);
                rest = Some(tail);
                Some(head)
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn language_switch_defaults_to_dutch() {
        assert_eq!(
            Lexicon::detached(Some("EN"), Some("NN"), None).len(),
            92_662
        );
        assert_eq!(Lexicon::detached(Some("DU"), Some("N"), None).len(), 11_699);
        assert_eq!(Lexicon::detached(None, Some("N"), None).len(), 11_699);
        assert_eq!(Lexicon::detached(Some("FR"), Some("N"), None).len(), 11_699);
    }

    #[test]
    fn nested_default_guard() {
        let l = Lexicon::detached(Some("EN"), None, Some("NNP"));
        assert!(l.default_category.is_none());
        assert!(
            l.default_category_capitalised.is_none(),
            "a falsy defaultCategory suppresses the capitalised one too"
        );
        let l = Lexicon::detached(Some("EN"), Some(""), Some("NNP"));
        assert!(l.default_category_capitalised.is_none());
        let l = Lexicon::detached(Some("EN"), Some("NN"), Some("NNP"));
        assert_eq!(l.default_category_capitalised.as_deref(), Some("NNP"));
    }

    #[test]
    fn set_default_categories_has_different_rules() {
        let mut l = Lexicon::detached(Some("EN"), Some("AAA"), Some("BBB"));
        // Unconditional for the first argument...
        l.set_default_categories(None, None);
        assert!(l.default_category.is_none());
        // ...but the second is never cleared.
        assert_eq!(l.default_category_capitalised.as_deref(), Some("BBB"));
        l.set_default_categories(Some(""), Some("CCC"));
        assert_eq!(l.default_category.as_deref(), Some(""));
        assert_eq!(l.default_category_capitalised.as_deref(), Some("CCC"));
    }

    #[test]
    fn empty_entry_beats_the_default_category() {
        let l = Lexicon::detached(Some("EN"), Some("NN"), None);
        assert_eq!(l.tag_word(""), Categories::Found(vec![]));
        assert_eq!(l.first_category(""), None, "no tag, not the default");
        // ...whereas a genuinely absent word does take the default.
        assert_eq!(l.first_category("zzzznotaword").as_deref(), Some("NN"));
    }

    #[test]
    fn proto_lookup_returns_the_prototype() {
        let l = Lexicon::detached(Some("EN"), Some("NN"), None);
        assert_eq!(l.tag_word("__proto__"), Categories::ObjectPrototype);
        assert_eq!(l.first_category("__proto__"), None);
        // Uppercased, the lowercase fallback finds it too.
        assert_eq!(l.tag_word("__PROTO__"), Categories::ObjectPrototype);
        // The function-valued prototype members behave exactly like a miss.
        assert_eq!(l.first_category("toString").as_deref(), Some("NN"));
        assert_eq!(l.first_category("hasOwnProperty").as_deref(), Some("NN"));
    }

    #[test]
    fn words_named_after_reference_values_are_real_entries() {
        let l = Lexicon::detached(Some("EN"), Some("NN"), None);
        assert_eq!(l.first_category("undefined").as_deref(), Some("JJ"));
        assert_eq!(l.first_category("null").as_deref(), Some("JJ"));
        assert_eq!(l.first_category("length").as_deref(), Some("NN"));
    }

    #[test]
    fn non_string_lookups_hit_or_throw_depending_on_the_language() {
        // `tagWord(undefined)` really does find an English entry, because the
        // dictionary has a word spelled "undefined". Dutch does not.
        let en = Lexicon::detached(Some("EN"), Some("NN"), None);
        assert_eq!(
            en.tag_word_value(LookupKey::Undefined),
            Ok(Categories::Found(vec![Cow::Borrowed("JJ")]))
        );
        assert_eq!(
            en.tag_word_value(LookupKey::Null),
            Ok(Categories::Found(vec![
                Cow::Borrowed("JJ"),
                Cow::Borrowed("NN")
            ]))
        );
        assert_eq!(
            en.tag_word_value(LookupKey::NonString("5")),
            Err(TaggerError::WordNotAString)
        );

        let du = Lexicon::detached(Some("DU"), Some("N"), None);
        assert_eq!(
            du.tag_word_value(LookupKey::Undefined),
            Err(TaggerError::undefined("toLowerCase"))
        );
        assert_eq!(
            du.tag_word_value(LookupKey::Null),
            Err(TaggerError::ReadOfUndefined {
                object: "null",
                property: "toLowerCase"
            })
        );
        // A string argument is just the ordinary path.
        assert_eq!(
            en.tag_word_value(LookupKey::Str("zzzzznotaword")),
            Ok(Categories::Default(Some(Cow::Borrowed("NN"))))
        );
    }

    #[test]
    fn defaults_are_ascii_capitalisation_only() {
        let l = Lexicon::detached(Some("EN"), Some("NN"), Some("NNP"));
        assert_eq!(l.tag_word_with_defaults("Zzzzz").as_deref(), Some("NNP"));
        assert_eq!(l.tag_word_with_defaults("zzzzz").as_deref(), Some("NN"));
        assert_eq!(l.tag_word_with_defaults("Ålesund").as_deref(), Some("NN"));
        assert_eq!(l.tag_word_with_defaults("Москва").as_deref(), Some("NN"));
        assert_eq!(l.tag_word_with_defaults("").as_deref(), Some("NN"));
    }

    #[test]
    fn parse_lexicon_shapes() {
        let mut l = Lexicon::detached(Some("EN"), Some("NN"), None);
        l.parse_lexicon("2 CD\nthe DT IN\ndog NN NNS\ncat").unwrap();
        assert_eq!(l.len(), 4);
        assert_eq!(
            l.pretty_print(),
            "2\tCD\t\nthe\tDT\tIN\t\ndog\tNN\tNNS\t\ncat\t\n"
        );
        // A single-token line stores an EMPTY list, which tag_word returns.
        assert_eq!(l.tag_word("cat"), Categories::Found(vec![]));
        assert_eq!(l.first_category("cat"), None);
        // Detached: the shared English dictionary is untouched.
        assert_eq!(Lexicon::detached(Some("EN"), None, None).len(), 92_662);
    }

    #[test]
    fn parse_lexicon_rejects_input_without_characters() {
        let mut l = Lexicon::detached(Some("EN"), None, None);
        for input in ["", "\n", "\n\n", "\r\n"] {
            assert_eq!(
                l.parse_lexicon(input),
                Err(TaggerError::ReadOfUndefined {
                    object: "null",
                    property: "forEach"
                }),
                "{input:?}"
            );
        }
        // A whitespace-only line does have characters, and yields the key "".
        let mut l = Lexicon::detached(Some("EN"), None, None);
        l.parse_lexicon("   \n").unwrap();
        assert_eq!(l.len(), 1);
        assert_eq!(l.pretty_print(), "\t\n");
    }

    #[test]
    fn parse_lexicon_splits_on_reference_whitespace() {
        let mut l = Lexicon::detached(Some("EN"), None, None);
        // U+FEFF is \s in the reference but not whitespace to Rust.
        l.parse_lexicon("x\u{feff}y").unwrap();
        assert_eq!(l.pretty_print(), "x\ty\t\n");
        // U+0085 is whitespace to Rust but NOT \s in the reference.
        let mut l = Lexicon::detached(Some("EN"), None, None);
        l.parse_lexicon("x\u{0085}y").unwrap();
        assert_eq!(l.pretty_print(), "x\u{0085}y\t\n");
    }

    #[test]
    fn runtime_numeric_keys_are_hoisted() {
        let mut l = Lexicon::empty();
        l.add_word("b", vec![Cow::Borrowed("NN")]);
        l.add_word("10", vec![Cow::Borrowed("CD")]);
        l.add_word("a", vec![Cow::Borrowed("NN")]);
        l.add_word("2", vec![Cow::Borrowed("CD")]);
        let mut keys = Vec::new();
        l.for_each_entry(&mut |k, _| keys.push(k.to_string()));
        assert_eq!(keys, ["2", "10", "b", "a"]);
    }

    #[test]
    fn overriding_a_base_key_keeps_its_position_and_count() {
        let mut l = Lexicon::detached(Some("DU"), None, None);
        let before = l.len();
        l.add_word("1", vec![Cow::Borrowed("ZZ")]);
        assert_eq!(l.len(), before, "an override is not a new key");
        let mut first = None;
        l.for_each_entry(&mut |k, t| {
            if first.is_none() {
                first = Some((
                    k.to_string(),
                    t.iter().map(|s| (*s).to_string()).collect::<Vec<_>>(),
                ));
            }
        });
        assert_eq!(first, Some(("1".to_string(), vec!["ZZ".to_string()])));
    }

    #[test]
    fn detached_instances_do_not_share() {
        let mut a = Lexicon::detached(Some("EN"), Some("NN"), None);
        let b = Lexicon::detached(Some("EN"), Some("NN"), None);
        a.add_word("zzzprivate", vec![Cow::Borrowed("XX")]);
        assert_eq!(a.first_category("zzzprivate").as_deref(), Some("XX"));
        assert_eq!(b.first_category("zzzprivate").as_deref(), Some("NN"));
    }

    #[test]
    fn assigning_proto_creates_no_key() {
        let mut l = Lexicon::empty();
        l.add_word("__proto__", vec![Cow::Borrowed("XX")]);
        assert_eq!(l.len(), 0);
        assert_eq!(l.tag_word("__proto__"), Categories::ObjectPrototype);
    }
}
