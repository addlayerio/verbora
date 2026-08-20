//! Beider-Morse Phonetic Matching (BMPM).
//!
//! # Provenance and licensing — read before touching `data/beider-morse/`
//!
//! The 127 rule files this module reads
//! (`crates/verbora-phonetics/data/beider-morse/`) are Apache-2.0-licensed
//! data copied from Apache Commons Codec — itself a Java port of Alexander
//! Beider and Stephen P. Morse's original, GPL-3.0-licensed PHP — and **not**
//! a copy of that PHP. See `data/beider-morse/NOTICE.md` for the full
//! provenance chain, the thirteen characters this crate restores where the
//! import lost them, the coverage gaps the corpus still has, and why
//! embedding Apache-2.0 data in this MIT-licensed crate is fine. Read it
//! before editing any rule file.
//!
//! The engine and parser here (`rule.rs`, `engine.rs`, this file) are
//! Verbora's own MIT-licensed Rust, written from the algorithm as described
//! by its authors — not a transliteration of any other implementation.
//!
//! `the_corpus_dead_rule_count_is_exactly_what_upstream_left_behind` below
//! records a real defect in that upstream corpus, with the reasoning for
//! leaving it in place. Read that test before assuming the tables are clean.
//!
//! User-facing prose lives on [`BeiderMorse`] and [`BeiderMorseCode`], not
//! here: this module is private, so a `//!` comment on it would not reach
//! docs.rs.

mod engine;
mod lang;
mod languages;
mod rule;

use std::borrow::Cow;
use std::collections::HashMap;
use std::sync::OnceLock;
use std::sync::RwLock;

pub use languages::{Language, LanguageSet};

use engine::{OnUnmatched, PhonemeBuilder, RuleTable, merge_by_text};
use rule::{Line, Rule, parse_line};

/// Which family-naming convention's rule tables to apply. This is a real
/// choice, not a formality: it picks which language-specific rule files exist
/// to draw from at all (Generic:
/// 18 languages: arabic, cyrillic, czech, dutch, english, french, german,
/// greek, greeklatin, hebrew, hungarian, italian, polish, portuguese,
/// romanian, russian, spanish, turkish. Ashkenazi: 10 of those. Sephardic:
/// 5), not merely a stricter/looser filter over one shared set.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum NameType {
    /// General-purpose rule set; the default recommendation absent a
    /// specific reason to choose Ashkenazi or Sephardic.
    Generic,
    /// Tuned for Ashkenazi Jewish family-name conventions.
    Ashkenazi,
    /// Tuned for Sephardic Jewish family-name conventions.
    Sephardic,
}

impl NameType {
    const fn file_prefix(self) -> &'static str {
        match self {
            Self::Generic => "gen",
            Self::Ashkenazi => "ash",
            Self::Sephardic => "sep",
        }
    }

    /// Leading words (`"van Beethoven"`, `"de la Cruz"`) that a name is
    /// conventionally split around rather than encoded as one run. The list is
    /// fixed per name type and is deliberately not derived from the rule
    /// corpus: which words are name prefixes is a naming-convention fact, not
    /// a phonetic one. Note `"de la"` is itself a two-word entry: the prefix
    /// check below matches it as one literal string against the *unsplit*
    /// input, before word-splitting happens.
    const fn name_prefixes(self) -> &'static [&'static str] {
        match self {
            Self::Generic => &[
                "da", "dal", "de", "del", "dela", "de la", "della", "des", "di", "do", "dos", "du",
                "van", "von",
            ],
            Self::Ashkenazi => &["bar", "ben", "da", "de", "van", "von"],
            Self::Sephardic => &[
                "al", "el", "da", "dal", "de", "del", "dela", "de la", "della", "des", "di", "do",
                "dos", "du", "van", "von",
            ],
        }
    }
}

/// How wide a net the final refinement pass casts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RuleType {
    /// Casts the widest net across plausible historical/cross-language
    /// spelling drift, which is what Beider-Morse offers over a
    /// single-language algorithm; choose it unless an exact-match index is
    /// the goal.
    Approx,
    /// A narrower refinement pass, closer to "how the name reads today" —
    /// smaller candidate sets, useful where an exact-match index is the
    /// goal rather than broad recall.
    Exact,
}

/// Internal: the "Rules" pass shares file-loading and rule-application
/// machinery with `Approx`/`Exact` but is never itself caller-selectable —
/// every [`encode`](BeiderMorse::encode) call runs it first, unconditionally,
/// before the caller-selected [`RuleType`]'s own refinement pass.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum PassKind {
    Rules,
    Refine(RuleType),
}

impl PassKind {
    const fn file_infix(self) -> &'static str {
        match self {
            Self::Rules => "rules",
            Self::Refine(RuleType::Approx) => "approx",
            Self::Refine(RuleType::Exact) => "exact",
        }
    }
}

include!("embedded_files.rs");

fn resolve_file(name: &str) -> &'static str {
    EMBEDDED_FILES
        .iter()
        .find(|(n, _)| *n == name)
        .unwrap_or_else(|| panic!("beider-morse: no embedded rule file named {name:?}"))
        .1
}

/// Every file opens with an Apache-2.0 `/* ... */` license header (see
/// `data/beider-morse/NOTICE.md`), and a few files use standalone `/* ... */`
/// blocks for longer explanatory notes elsewhere in the file. Every
/// occurrence observed in the real corpus opens and closes on its own line
/// (`/*` alone, `*/` alone, content lines in between starting with ` * `) —
/// this walks lines with that one bit of state, never allocating or
/// rewriting the source text, so the yielded lines still borrow directly
/// from the original `'static` embedded string.
fn meaningful_lines(text: &'static str) -> impl Iterator<Item = &'static str> {
    let mut in_block_comment = false;
    text.lines().filter(move |line| {
        let trimmed = line.trim();
        if in_block_comment {
            if trimmed.ends_with("*/") {
                in_block_comment = false;
            }
            false
        } else if trimmed.starts_with("/*") {
            in_block_comment = !trimmed.ends_with("*/");
            false
        } else {
            true
        }
    })
}

/// Parses one rule file, recursively splicing `#include`d files' lines in
/// at the exact point the directive appears — matching order is
/// significant (the first matching rule in a bucket wins), so this must
/// preserve it exactly rather than, say, processing includes after the
/// file's own rules.
fn load_rule_lines(filename: &str, out: &mut Vec<rule::RawRule<'static>>) {
    let text = resolve_file(filename);
    for line in meaningful_lines(text) {
        match parse_line(line) {
            Some(Line::Rule(r)) => out.push(r),
            Some(Line::Include(name)) => load_rule_lines(name, out),
            Some(Line::Word(_)) | None => {}
        }
    }
}

/// Parses a `*_languages.txt` file into its language name list, in file
/// order (order is the [`Language`] index assignment: `any` — the pseudo-
/// language file selector, never a rule tag — is always index 0 and
/// carries no [`Language`] value of its own, so the first *real* language
/// name is `Language(0)`).
fn load_language_names(name_type: NameType) -> Vec<&'static str> {
    let filename = format!("{}_languages", name_type.file_prefix());
    let text = resolve_file(&filename);
    meaningful_lines(text)
        .filter_map(parse_line)
        .filter_map(|line| match line {
            Line::Word(w) if w != "any" => Some(w),
            _ => None,
        })
        .collect()
}

/// A compiled rule table for one `(NameType, PassKind, language-file-suffix)`
/// key, plus the resolved language index it was compiled against.
struct CompiledTable {
    table: RuleTable,
}

/// Per-`NameType` state: its language name table (name -> index, and the
/// total count needed to build [`LanguageSet::all`]) plus a cache of
/// compiled rule tables, populated lazily — encoding with only the `"any"`
/// language never compiles a single language-specific file, and using one
/// real language never touches the other seventeen.
struct NameTypeData {
    language_names: Vec<&'static str>,
    lang_guesser: lang::LangGuesser,
    tables: RwLock<HashMap<(PassKind, &'static str), std::sync::Arc<CompiledTable>>>,
}

impl NameTypeData {
    fn new(name_type: NameType) -> Self {
        let language_names = load_language_names(name_type);
        let all_languages = LanguageSet::all(language_names.len() as u8);
        let resolve = {
            let language_names = language_names.clone();
            move |name: &str| {
                language_names
                    .iter()
                    .position(|&n| n == name)
                    .map(|i| LanguageSet::single(Language(i as u8)))
            }
        };
        let lang_filename = format!("{}_lang", name_type.file_prefix());
        let lang_guesser = lang::compile(resolve_file(&lang_filename), all_languages, resolve);
        Self {
            language_names,
            lang_guesser,
            tables: RwLock::new(HashMap::new()),
        }
    }

    /// Guesses which of this name type's languages `word` is plausibly
    /// spelled under, purely from its spelling — see [`lang::LangGuesser`].
    fn guess_languages(&self, word: &str) -> LanguageSet {
        self.lang_guesser.guess(word)
    }

    fn all_languages(&self) -> LanguageSet {
        LanguageSet::all(self.language_names.len() as u8)
    }

    fn resolve_language_name(&self, name: &str) -> Option<LanguageSet> {
        self.language_names
            .iter()
            .position(|&n| n == name)
            .map(|i| LanguageSet::single(Language(i as u8)))
    }

    /// Like [`Self::resolve_language_name`], but also returns the matched
    /// name as the `'static` string from this table (not the caller's own
    /// borrowed `&str`) — needed wherever the name is used as a rule-file
    /// suffix cached under a `'static` key (see [`Self::table`]).
    fn resolve_language_file(&self, name: &str) -> Option<(&'static str, LanguageSet)> {
        self.language_names
            .iter()
            .position(|&n| n == name)
            .map(|i| {
                (
                    self.language_names[i],
                    LanguageSet::single(Language(i as u8)),
                )
            })
    }

    /// The compiled table for `pass` restricted to `language_file_suffix`
    /// (a real language name, or `"any"`/`"common"` for the shared files),
    /// compiling and caching it on first use.
    fn table(
        &self,
        name_type: NameType,
        pass: PassKind,
        language_file_suffix: &'static str,
    ) -> std::sync::Arc<CompiledTable> {
        let key = (pass, language_file_suffix);
        // Poison is recovered, not propagated. `BeiderMorse`'s own type
        // documentation promises "Total: no input panics", and propagating
        // poison would break that promise for *every later call in the
        // process* the moment one caller panicked anywhere near this lock —
        // the exact defect `verbora_stemmers::stopwords` already found and
        // removed (see its `a_poisoned_lock_does_not_take_the_language_down_
        // with_it`). Recovery is sound here because the data behind the lock
        // is a pure cache of compiled tables: a panic cannot leave it half
        // written, since the only mutation is the single `insert` below, and
        // the rule compilation that *can* panic runs outside both guards.
        if let Some(t) = self
            .tables
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get(&key)
        {
            return std::sync::Arc::clone(t);
        }
        let filename = format!(
            "{}_{}_{}",
            name_type.file_prefix(),
            pass.file_infix(),
            language_file_suffix
        );
        let mut raw = Vec::new();
        load_rule_lines(&filename, &mut raw);
        let all_languages = self.all_languages();
        let mut by_first_char: HashMap<char, Vec<Rule>> = HashMap::new();
        for raw_rule in &raw {
            let compiled = Rule::compile(raw_rule, all_languages, |name| {
                self.resolve_language_name(name)
            })
            .unwrap_or_else(|e| {
                panic!(
                    "beider-morse: bad regex in {filename}: {e} (rule pattern {:?})",
                    raw_rule.pattern
                )
            });
            // Key off the *compiled* pattern, never `raw_rule.pattern`.
            // The two differ exactly where the DSL's one escape is used:
            // `Rule::compile` unescapes `\"` to `"`, and `Rule::matches`
            // then matches on `"` -- so keying off the raw field would file
            // those rules under `\` and no lookup could ever reach them.
            // `by_first_char` is an index into rules, and an index must be
            // built with the same spelling it will be consulted with.
            // Pinned by `every_rule_is_bucketed_under_its_compiled_patterns_first_character`.
            let ch = compiled.pattern.chars().next().unwrap_or('\u{0}');
            by_first_char.entry(ch).or_default().push(compiled);
        }
        let compiled = std::sync::Arc::new(CompiledTable {
            table: RuleTable { by_first_char },
        });
        self.tables
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(key, std::sync::Arc::clone(&compiled));
        compiled
    }
}

fn name_type_data(name_type: NameType) -> &'static NameTypeData {
    static GENERIC: OnceLock<NameTypeData> = OnceLock::new();
    static ASHKENAZI: OnceLock<NameTypeData> = OnceLock::new();
    static SEPHARDIC: OnceLock<NameTypeData> = OnceLock::new();
    let cell = match name_type {
        NameType::Generic => &GENERIC,
        NameType::Ashkenazi => &ASHKENAZI,
        NameType::Sephardic => &SEPHARDIC,
    };
    cell.get_or_init(|| NameTypeData::new(name_type))
}

/// One name's Beider-Morse encoding: every plausible phonetic spelling
/// [`BeiderMorse::encode`] produced, deduplicated by text.
///
/// # Why this is not [`PhoneticCodes`](crate::PhoneticCodes)
///
/// Every other encoder in this crate produces exactly one code or two, which
/// is what `PhoneticCodes`'s `One`/`Two` shape is built around. Beider-Morse's
/// output is a genuinely *variable-length* set — `"Renault"` alone yields
/// eight alternatives under Generic/Approx — bounded only by the encoder's
/// `max_phonemes` cap (default 20). Forcing it into `PhoneticCodes` would
/// either silently drop real candidates, which for a blocking key means
/// *missing* matches, or widen every other encoder's shape to carry a
/// cardinality only this one needs. It gets its own type instead, and
/// deliberately does not implement
/// [`PhoneticEncoder`](crate::PhoneticEncoder) — see that trait for the
/// alternative.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BeiderMorseCode {
    /// Independent candidate spellings, deduplicated by text and merged by
    /// language set, in alphabetical order (a side effect of the
    /// dedup/merge step going through a `BTreeMap` — deterministic, but not
    /// a ranking: there is no "better" candidate among these, only "still
    /// plausible under some subset of the requested languages"). **Unless
    /// [`Self::compound`] is `true`** — see that field before treating
    /// every element here as an independent candidate.
    pub spellings: Vec<String>,
    /// `true` for a `d'`-prefix, name-prefix, or (with `concat: false`)
    /// multi-word result — see [`BeiderMorse::encode`]'s own doc comment
    /// for when each case applies. When `true`, [`Self::spellings`] holds
    /// exactly one already-composed string like `"(krus|crus)-(dilakrus)"`
    /// (the parentheses, pipes and hyphens are literal characters of that one
    /// string: each parenthesised group is one part of the name, and the pipes
    /// separate that part's own candidates), not independent candidates —
    /// treat it as one opaque compound key, not a set to iterate. When `false`
    /// (the common case), every element of `spellings` is an independent
    /// candidate as usual.
    pub compound: bool,
}

/// Beider-Morse Phonetic Matching — every plausible spelling of a name,
/// across the languages that might have transcribed it.
///
/// # Publication
///
/// Alexander Beider and Stephen P. Morse, *Beider-Morse Phonetic Matching: An
/// Alternative to Soundex with Fewer False Hits*, and the rule corpus they
/// publish with it. Verbora reads the Apache Commons Codec transcription of
/// that corpus; see this crate's `data/beider-morse/NOTICE.md` for the
/// licensing chain.
///
/// # The problem it solves that the other encoders do not
///
/// [`DaitchMokotoff`](crate::DaitchMokotoff) covers Slavic, Germanic and
/// Ashkenazi surname matching with one fixed rule table. Beider-Morse solves
/// a harder problem: the *same* historical family name plausibly has
/// different phonetically-equivalent spellings depending on **which
/// country's** orthographic conventions transcribed it. A name carried from
/// Russia through Poland to Germany accumulates several "correct" spellings,
/// not one. The rule tables are therefore per-language, and the engine either
/// targets one language's conventions ([`BeiderMorse::encode_language`]) or
/// blends the `any`-language fallback rules to hedge across all of them
/// ([`BeiderMorse::encode`]).
///
/// # The contract
///
/// * **The text unit is one Unicode scalar.** The rule patterns are the
///   corpus's own, and they include accented Latin vowels, so unlike this
///   crate's Latin-alphabet encoders this one reads `è é ò ó` as themselves.
/// * The output is a **variable-length candidate list**, not one key or two —
///   see [`BeiderMorseCode`].
/// * **Total**: no input panics, and there is no error type. A word no rule
///   covers yields an empty candidate list.
///
/// # Scope
///
/// `encode`/`encode_language` **generate** candidate spellings. They do not
/// rank them, do not apply an edit-distance threshold, and do not index
/// anything — compose with [`PhoneticIndex`](crate::PhoneticIndex) or
/// `verbora-distance` at the call site, the same boundary
/// [`PhoneticIndex::neighbors`](crate::PhoneticIndex::neighbors) draws.
///
/// # Cost
///
/// Cheap to construct and cheap to copy: the value holds only a few small
/// fields, and the rule tables are parsed once per [`NameType`] and cached
/// process-wide rather than per instance, so building, cloning or dropping an
/// encoder never touches them. The first encode of a given `NameType` pays
/// the parse.
#[derive(Debug, Clone, Copy)]
pub struct BeiderMorse {
    name_type: NameType,
    rule_type: RuleType,
    max_phonemes: usize,
    concat: bool,
}

const DEFAULT_MAX_PHONEMES: usize = 20;

impl BeiderMorse {
    /// A Beider-Morse encoder for `name_type`/`rule_type`, with a candidate
    /// cap of 20 and `concat` on (see [`Self::with_concat`] for what that
    /// second default decides).
    #[must_use]
    pub const fn new(name_type: NameType, rule_type: RuleType) -> Self {
        Self {
            name_type,
            rule_type,
            max_phonemes: DEFAULT_MAX_PHONEMES,
            concat: true,
        }
    }

    /// Sets whether a multi-word name is fused into one run before encoding
    /// (`concat: true`, the default — `"jean paul"` is encoded as the
    /// single string `"jean paul"`, producing one cross-product candidate
    /// set that spans both words) or each word is encoded independently and
    /// hyphen-joined (`concat: false` — `"jean paul"` becomes
    /// `jean`'s-own-candidates `-` `paul`'s-own-candidates, each word
    /// re-guessing its own language). Which one is "more correct" depends
    /// on the caller: `concat: true` matches names that read as one
    /// phonetic unit; `concat: false` keeps a name's words independently
    /// comparable (useful when a middle name might be present on one side
    /// of a match and absent on the other).
    #[must_use]
    pub const fn with_concat(mut self, concat: bool) -> Self {
        self.concat = concat;
        self
    }

    /// Encodes `word`, first guessing which language(s) it's plausibly
    /// spelled under from the spelling itself; [`Self::encode_language`] is
    /// the variant that takes the caller's word for it instead. The guess is
    /// the `*_lang.txt` heuristic layer: it starts from every language this
    /// [`NameType`] has and narrows one rule at a time, in file order, an
    /// "accept" rule intersecting the running guess down to just its own
    /// listed languages when its pattern matches and a "reject" rule removing
    /// its listed languages instead. A guess that narrows all the way to
    /// nothing falls back to the full set, since "no candidate spelling is
    /// plausible under any language" is never the intent of the heuristic. A
    /// confident single-language guess (e.g. `"Renault"` → French) loads
    /// that language's own rule file and starts every candidate phoneme
    /// pre-filtered to it; an ambiguous guess falls back to the `"any"` file
    /// with the (possibly still narrowed) guessed set as the starting
    /// languages, same as [`Self::encode_language`] never running.
    ///
    /// Also handles the compound surname shapes: a leading `d'` (`"d'Angelo"`)
    /// or a [`NameType`]-specific name prefix (`"van Gogh"`, `"de la Cruz"` —
    /// Generic only) splits the name into `(without-the-prefix)-(with-the-
    /// prefix-fused-on)`, each re-encoded independently; a name with more
    /// than one word (and [`Self::with_concat`]`(false)`) encodes each word on
    /// its own and hyphen-joins the results, rather than treating the whole
    /// string as one phonetic run. In both cases the returned
    /// [`BeiderMorseCode::compound`] is `true` and `spellings` holds exactly
    /// one already-composed string rather than independent candidates — check
    /// `compound` before iterating `spellings` as a candidate set; see that
    /// field's own doc comment.
    #[must_use]
    pub fn encode(&self, word: &str) -> BeiderMorseCode {
        self.encode_top(word, None)
    }

    /// Encodes `word` restricted to the single named language (e.g.
    /// `"russian"`, `"english"`) — a smaller candidate set than
    /// [`Self::encode`], since only that language's own rule file (plus the
    /// shared `common` rules) is consulted, and every phoneme is
    /// pre-filtered to just that language throughout. Prefix- and
    /// multi-word-splitting (see [`Self::encode`]) still apply, and the
    /// *split-off parts* are always re-guessed from scratch rather than
    /// inheriting `language` — only the base case (a single already-split
    /// word) actually restricts to it.
    ///
    /// Returns `None` if `language` is not one of this encoder's
    /// [`NameType`]'s own known languages.
    #[must_use]
    pub fn encode_language(&self, word: &str, language: &str) -> Option<BeiderMorseCode> {
        name_type_data(self.name_type).resolve_language_file(language)?;
        Some(self.encode_top(word, Some(language)))
    }

    /// Normalizes `word`, then dispatches to prefix-splitting, multi-word
    /// splitting, or the single-word base case — the shared orchestration
    /// [`Self::encode`] and [`Self::encode_language`] both funnel through,
    /// differing only in whether the base case's language is guessed
    /// (`explicit_language: None`) or caller-chosen (`Some`).
    fn encode_top(&self, word: &str, explicit_language: Option<&str>) -> BeiderMorseCode {
        let normalized = word.to_lowercase().replace('-', " ");

        // `PhonemeBuilder::apply` (`engine.rs`) rebuilds each candidate's
        // whole accumulated text on every rule match -- the same
        // candidate-building shape every reference implementation uses --
        // so encoding one word alone already costs quadratically in its
        // length; an independent audit measured a single 64,000-character
        // word at 1.6s. No real person's name is anywhere close to this
        // cap, so a hard truncation here costs nothing for legitimate
        // input while bounding the worst case for adversarial or corrupted
        // input reaching the fully public `encode`/`encode_language`.
        const MAX_INPUT_CHARS: usize = 512;
        let normalized: Cow<'_, str> = if normalized.chars().count() > MAX_INPUT_CHARS {
            Cow::Owned(normalized.chars().take(MAX_INPUT_CHARS).collect())
        } else {
            Cow::Owned(normalized)
        };

        // A repeated name prefix (`"de de de ... cruz"`) recurses through
        // `combine_prefix_split` -> `self.encode` once per repetition,
        // and unlike ordinary multi-word splitting, `combined` at each
        // level is roughly the *same* length as the level above it, not
        // shrinking -- so recursion depth and per-level cost both scale
        // with input length at once. The same audit measured this
        // separately: ~600 chars of a repeated Generic prefix already
        // costs ~150ms, ~3,000 chars costs 14+ seconds -- far worse than
        // the flat truncation above alone would bound, since each
        // recursion level re-triggers the quadratic single-word cost on a
        // string nearly as long as the original. Skipping prefix-splitting
        // above this (smaller) cap falls through to ordinary multi-word
        // handling instead, which is non-recursive under the default
        // `concat: true` and bounded by word count under `concat: false`
        // (splitting a string into words can only ever *reduce* total
        // quadratic-in-length cost, never multiply it).
        const MAX_PREFIX_SPLIT_CHARS: usize = 128;

        if self.name_type == NameType::Generic
            && normalized.chars().count() <= MAX_PREFIX_SPLIT_CHARS
        {
            if let Some(remainder) = normalized.strip_prefix("d'") {
                let mut combined = String::with_capacity(remainder.len() + 1);
                combined.push('d');
                combined.push_str(remainder);
                return self.combine_prefix_split(remainder, &combined);
            }
            for prefix in self.name_type.name_prefixes() {
                let mut with_space = String::with_capacity(prefix.len() + 1);
                with_space.push_str(prefix);
                with_space.push(' ');
                if let Some(remainder) = normalized.strip_prefix(with_space.as_str()) {
                    let mut combined = String::with_capacity(prefix.len() + remainder.len());
                    combined.push_str(prefix);
                    combined.push_str(remainder);
                    return self.combine_prefix_split(remainder, &combined);
                }
            }
        }

        let words: Vec<&str> = normalized.split_whitespace().collect();

        if !self.concat && words.len() != 1 {
            let joined = words
                .iter()
                .map(|w| self.encode(w).spellings.join("|"))
                .collect::<Vec<_>>()
                .join("-");
            return BeiderMorseCode {
                spellings: vec![joined],
                compound: true,
            };
        }

        let single_word = if self.concat {
            words
                .iter()
                .copied()
                .map(|w| {
                    if self.name_type == NameType::Sephardic {
                        w.rsplit('\'').next().unwrap_or(w)
                    } else {
                        w
                    }
                })
                .filter(|w| {
                    self.name_type == NameType::Generic
                        || !self.name_type.name_prefixes().contains(w)
                })
                .collect::<Vec<_>>()
                .join(" ")
        } else {
            words.first().copied().unwrap_or("").to_string()
        };

        let data = name_type_data(self.name_type);
        let (language_file_suffix, languages) = match explicit_language {
            Some(language) => data
                .resolve_language_file(language)
                .expect("validated by encode_language before reaching encode_top"),
            None => {
                let guessed = data.guess_languages(&single_word);
                let suffix = guessed
                    .as_singleton()
                    .map(|language| data.language_names[language.index() as usize])
                    .unwrap_or("any");
                (suffix, guessed)
            }
        };
        self.encode_for_language_file(&single_word, language_file_suffix, languages)
    }

    /// Builds the `"(without-prefix)-(with-prefix)"` compound result shared
    /// by the `d'`- and name-prefix cases in [`Self::encode_top`] — both
    /// re-derive their language from scratch for each half (see
    /// [`Self::encode`]'s own doc comment).
    fn combine_prefix_split(&self, remainder: &str, combined: &str) -> BeiderMorseCode {
        let remainder_code = self.encode(remainder);
        let combined_code = self.encode(combined);
        let text = format!(
            "({})-({})",
            remainder_code.spellings.join("|"),
            combined_code.spellings.join("|")
        );
        BeiderMorseCode {
            spellings: vec![text],
            compound: true,
        }
    }

    fn encode_for_language_file(
        &self,
        word: &str,
        language_file_suffix: &'static str,
        languages: LanguageSet,
    ) -> BeiderMorseCode {
        let data = name_type_data(self.name_type);

        let rules_table = data.table(self.name_type, PassKind::Rules, language_file_suffix);
        let mut builder = PhonemeBuilder::empty(languages);
        rules_table
            .table
            .apply_to(word, &mut builder, self.max_phonemes, OnUnmatched::Skip);

        let common_final = data.table(self.name_type, PassKind::Refine(self.rule_type), "common");
        let specific_final = data.table(
            self.name_type,
            PassKind::Refine(self.rule_type),
            language_file_suffix,
        );

        let refined = apply_final_pass(
            &builder.into_candidates(),
            &common_final.table,
            self.max_phonemes,
        );
        let refined = apply_final_pass(&refined, &specific_final.table, self.max_phonemes);

        let merged = merge_by_text(refined);
        BeiderMorseCode {
            spellings: merged.into_iter().map(|p| p.text).collect(),
            compound: false,
        }
    }
}

/// Runs one final-rule table over every candidate's own phoneme *text*
/// (not the original word): the refinement pass operates on what the Rules
/// pass already produced, which is the two-stage shape the rule corpus is
/// written for: its `*_approx_*`/`*_exact_*` files match phoneme text, not
/// spelling. `engine.rs` holds the rule-application machinery both passes
/// share.
fn apply_final_pass(
    candidates: &[engine::Phoneme],
    table: &RuleTable,
    max_phonemes: usize,
) -> Vec<engine::Phoneme> {
    if table.by_first_char.is_empty() {
        return candidates.to_vec();
    }
    let mut out = Vec::new();
    for candidate in candidates {
        let mut sub = PhonemeBuilder::empty(candidate.languages);
        table.apply_to(
            &candidate.text,
            &mut sub,
            max_phonemes,
            OnUnmatched::PassThrough,
        );
        out.extend(sub.into_candidates());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generic_any_encodes_a_known_word() {
        let bm = BeiderMorse::new(NameType::Generic, RuleType::Approx);
        let code = bm.encode("Renault");
        // The specification here is the rule corpus this crate ships: language
        // guessing picks a singleton French match, and applying the French
        // approx rules to "Renault" yields these eight candidates. That makes
        // the expectation derivable from data in the repository rather than
        // from any implementation's output -- `every_rule_pattern_is_reachable`
        // and the corpus-integrity tests are what keep that data honest.
        //
        // Sorted before comparing because the engine's candidate order is not
        // part of the contract; the set is.
        let mut spellings = code.spellings;
        spellings.sort();
        assert_eq!(
            spellings,
            vec![
                "rinD", "rinDlt", "rina", "rinalt", "rino", "rinolt", "rinu", "rinult"
            ]
        );
    }

    #[test]
    fn unknown_language_name_is_none() {
        let bm = BeiderMorse::new(NameType::Generic, RuleType::Approx);
        assert!(bm.encode_language("Renault", "klingon").is_none());
    }

    #[test]
    fn known_language_name_is_some() {
        let bm = BeiderMorse::new(NameType::Generic, RuleType::Approx);
        assert!(bm.encode_language("Renault", "french").is_some());
    }

    #[test]
    fn apostrophe_prefix_produces_a_compound_result() {
        let bm = BeiderMorse::new(NameType::Generic, RuleType::Approx);
        let code = bm.encode("d'Angelo");
        // The `d'`-prefix case always returns exactly one already-composed
        // "(without-prefix)-(with-prefix)" string, matching the reference
        // implementations' own single-`String` return shape for this case
        // (see `BeiderMorse::encode`'s own doc comment) -- cross-checked
        // verbatim against the oracle during development.
        assert!(code.compound);
        assert_eq!(code.spellings.len(), 1);
        assert!(code.spellings[0].starts_with('('));
        assert!(code.spellings[0].contains(")-("));
    }

    #[test]
    fn name_prefix_produces_a_compound_result() {
        let bm = BeiderMorse::new(NameType::Generic, RuleType::Approx);
        let code = bm.encode("von Neumann");
        assert!(code.compound);
        assert_eq!(code.spellings.len(), 1);
        assert!(code.spellings[0].contains(")-("));
    }

    #[test]
    fn non_generic_name_prefix_is_dropped_not_split() {
        // Unlike Generic, Ashkenazi/Sephardic never build a compound
        // "(without)-(with)" result for a leading name-prefix word --
        // `encode_top`'s prefix-splitting `if` only ever runs for
        // `NameType::Generic`. Instead, under the default `concat: true`,
        // the prefix word is filtered out of the fused input entirely (see
        // the `.filter` in `encode_top`), so it contributes nothing at all
        // to the result -- real information loss, not an alternative
        // encoding. Pinned here because it's easy to assume by analogy with
        // Generic that every `NameType` treats a name prefix the same way.
        let ash = BeiderMorse::new(NameType::Ashkenazi, RuleType::Approx);
        let with_prefix = ash.encode("ben Gurion");
        let without_prefix = ash.encode("Gurion");
        assert!(!with_prefix.compound);
        assert_eq!(with_prefix, without_prefix);
    }

    #[test]
    fn multi_word_concat_defaults_to_fusing_into_one_lookup() {
        let bm = BeiderMorse::new(NameType::Generic, RuleType::Approx);
        // `concat` defaults to `true` (every reference implementation's own
        // real default, despite its own doc comment claiming otherwise --
        // confirmed against the constructor source, see `BeiderMorse::new`'s
        // own doc comment): "Jean Paul" is encoded as one fused lookup
        // (going through the same base case a single word would, so it
        // still returns a normal multi-candidate set), not hyphen-joined
        // per word, so no candidate contains a literal '-'.
        let code = bm.encode("Jean Paul");
        assert!(!code.compound);
        assert!(code.spellings.len() > 1);
        assert!(code.spellings.iter().all(|s| !s.contains('-')));
        // The Rules pass must silently skip the space it fused the two
        // words around (see `engine::OnUnmatched`) rather than embedding it.
        assert!(code.spellings.iter().all(|s| !s.contains(' ')));
    }

    #[test]
    fn multi_word_split_hyphenates_independent_per_word_results() {
        let bm = BeiderMorse::new(NameType::Generic, RuleType::Approx).with_concat(false);
        let code = bm.encode("Jean Paul");
        assert!(code.compound);
        assert_eq!(code.spellings.len(), 1);
        assert!(code.spellings[0].contains('-'));
    }

    #[test]
    fn ashkenazi_and_sephardic_encode_exact_values() {
        // Both cross-checked verbatim against the oracle during development
        // (see `AGENTS.md`'s Beider-Morse section for the full 10+10 sweep
        // this is drawn from) -- exact values, not just non-empty, so a
        // regression in either NameType's own rule-file wiring is caught.
        let ash = BeiderMorse::new(NameType::Ashkenazi, RuleType::Approx);
        let mut spellings = ash.encode("Cohen").spellings;
        spellings.sort();
        assert_eq!(spellings, vec!["kYin", "koin"]);

        let sep = BeiderMorse::new(NameType::Sephardic, RuleType::Approx);
        let mut spellings = sep.encode("Toledano").spellings;
        spellings.sort();
        assert_eq!(spellings, vec!["tulidana", "tulidanu"]);
    }

    #[test]
    fn repeated_name_prefix_does_not_blow_up() {
        // Regression test for a real finding from an independent audit: a
        // repeated Generic name prefix (`"de de de ... cruz"`) used to
        // recurse through `combine_prefix_split` once per repetition, each
        // level costing roughly as much as the original input -- measured
        // at 14+ seconds for ~3,000 characters before `encode_top`'s
        // `MAX_PREFIX_SPLIT_CHARS` guard was added. This must stay well
        // under a second; the test would otherwise hang the whole suite.
        let bm = BeiderMorse::new(NameType::Generic, RuleType::Approx);
        let attack: String = "de ".repeat(1000) + "cruz";
        let start = std::time::Instant::now();
        let code = bm.encode(&attack);
        assert!(
            start.elapsed() < std::time::Duration::from_secs(2),
            "took {:?}, expected the length guard to keep this fast",
            start.elapsed()
        );
        assert!(!code.spellings.is_empty());
    }

    #[test]
    fn corpus_language_tags_all_resolve() {
        // `Rule::compile` silently drops any phoneme alternative whose
        // language tag doesn't resolve, with no warning of any kind (see
        // `Rule::compile`'s own doc comment) -- if *every* alternative on a
        // rule failed to resolve, that rule would collapse the whole
        // in-progress candidate branch to nothing, silently. This is the
        // permanent regression guard that the real embedded corpus never
        // actually hits that: every `[language]` or `[lang1+lang2+...]` tag
        // on every phonetic-output field, across every rule file for every
        // `NameType`, must resolve against that `NameType`'s own language
        // list.
        for (name_type, prefix) in [
            (NameType::Generic, "gen"),
            (NameType::Ashkenazi, "ash"),
            (NameType::Sephardic, "sep"),
        ] {
            let data = name_type_data(name_type);
            for &(filename, text) in EMBEDDED_FILES {
                if !filename.starts_with(prefix)
                    || filename.ends_with("_languages")
                    || filename.ends_with("_lang")
                {
                    continue;
                }
                for line in meaningful_lines(text) {
                    let Some(Line::Rule(raw)) = parse_line(line) else {
                        continue;
                    };
                    for alt in rule::parse_phoneme_expr(raw.phonetic).0 {
                        let Some(tag) = alt.language else { continue };
                        for name in tag.split('+') {
                            assert!(
                                data.resolve_language_name(name).is_some(),
                                "unresolvable language {name:?} (tag {tag:?}) in {filename} for {name_type:?}"
                            );
                        }
                    }
                }
            }
        }
    }

    /// The four accented-vowel rules `*_rules_italian.txt` carries, and the
    /// unaccented vowel each is phonetically identical to. Shared by the
    /// tests below so the set is stated once and every one of them
    /// enumerates all four rather than sampling.
    const ITALIAN_ACCENTED_VOWELS: [(char, char); 4] =
        [('é', 'e'), ('è', 'e'), ('ó', 'o'), ('ò', 'o')];

    #[test]
    fn italian_accented_vowels_are_encoded_not_dropped() {
        // `gen_rules_italian.txt` and `sep_rules_italian.txt` each carry
        // `"é" "" "" "e"`, `"è" "" "" "e"`, `"ó" "" "" "o"` and
        // `"ò" "" "" "o"` -- so each accented vowel hands the Rules pass
        // exactly the phoneme its unaccented twin does. The Rules pass's
        // output text is the *only* input the Approx/Exact passes ever see
        // (see `encode_for_language_file`), so identical Rules-pass text
        // forces identical final output whatever those later passes do:
        // the accented and unaccented spellings must encode alike.
        //
        // `tr` + vowel is deliberate. No rule in either Italian file gives
        // `t` or `r` a left or right context, and the vowel is word-final,
        // so swapping the vowel cannot change how any other position is
        // scanned -- the equality is a property of the vowel rules alone.
        //
        // Enumerated over all four vowels x both name types x both rule
        // types: when the corpus carried U+FFFD for all four patterns they
        // collapsed into a single `by_first_char` bucket, and since
        // `RuleTable::apply_to` takes the first matching rule in a bucket,
        // three of the four could never fire even had the shared pattern
        // been a real character.
        for (accented, plain) in ITALIAN_ACCENTED_VOWELS {
            for name_type in [NameType::Generic, NameType::Sephardic] {
                for rule_type in [RuleType::Approx, RuleType::Exact] {
                    let bm = BeiderMorse::new(name_type, rule_type);
                    let accented_word = format!("tr{accented}");
                    let plain_word = format!("tr{plain}");
                    let with_accent = bm
                        .encode_language(&accented_word, "italian")
                        .expect("italian is a language of every name type tested here");
                    let without_accent = bm
                        .encode_language(&plain_word, "italian")
                        .expect("italian is a language of every name type tested here");
                    let vowel_deleted = bm
                        .encode_language("tr", "italian")
                        .expect("italian is a language of every name type tested here");
                    assert_eq!(
                        with_accent.spellings, without_accent.spellings,
                        "{name_type:?}/{rule_type:?}: {accented_word:?} must encode exactly as \
                         {plain_word:?}"
                    );
                    assert_ne!(
                        with_accent.spellings, vowel_deleted.spellings,
                        "{name_type:?}/{rule_type:?}: {accented:?} contributed nothing to \
                         {accented_word:?} -- the Rules pass matched no rule and dropped it"
                    );
                }
            }
        }
    }

    #[test]
    fn italian_accented_vowels_have_hand_derived_exact_encodings() {
        // Exact values walked by hand through the rule files, not recorded
        // from this implementation.
        //
        // Rules pass (`gen_rules_italian.txt`): `t` -> `t` and `r` -> `r`
        // (the catch-all latin-alphabet rules, no contexts), `é`/`è` -> `e`,
        // `ó`/`ò` -> `o`. So `trè` and `tré` leave the Rules pass as `tre`,
        // `trò` and `tró` as `tro`.
        //
        // Exact pass 1 (`gen_exact_common.txt`, plus the
        // `gen_exact_approx_common.txt` it `#include`s): every `t` rule
        // there requires a right context of `[vbgZz]`, `d` or `t`; the only
        // `r` rule requires a following `r`; there is no `e` or `o` rule at
        // all. None of those contexts hold in `tre`/`tro`, so with
        // `OnUnmatched::PassThrough` every character survives unchanged.
        //
        // Exact pass 2 (`gen_exact_italian.txt`) is empty, so
        // `apply_final_pass` returns its input untouched.
        let bm = BeiderMorse::new(NameType::Generic, RuleType::Exact);
        for (word, expected) in [
            ("tré", "tre"),
            ("trè", "tre"),
            ("tró", "tro"),
            ("trò", "tro"),
        ] {
            let code = bm
                .encode_language(word, "italian")
                .expect("italian is a Generic language");
            assert!(!code.compound, "{word:?} is a single word, not a compound");
            assert_eq!(code.spellings, vec![expected.to_owned()], "{word:?}");
        }
    }

    #[test]
    fn each_italian_accented_vowel_rule_gets_its_own_reachable_bucket() {
        // The structural half of the defect: four rules sharing one pattern
        // share one bucket, and `RuleTable::apply_to` stops at the first
        // rule in a bucket that matches -- so restoring the bytes is only a
        // fix if it also makes the four patterns distinct. Each accented
        // vowel must key a bucket of its own, holding exactly the one rule,
        // carrying exactly the one phoneme.
        for name_type in [NameType::Generic, NameType::Sephardic] {
            let data = name_type_data(name_type);
            let compiled = data.table(name_type, PassKind::Rules, "italian");
            for (accented, plain) in ITALIAN_ACCENTED_VOWELS {
                let bucket = compiled
                    .table
                    .by_first_char
                    .get(&accented)
                    .unwrap_or_else(|| panic!("{name_type:?}: no bucket for {accented:?}"));
                assert_eq!(
                    bucket.len(),
                    1,
                    "{name_type:?}: {accented:?} shares its bucket with {} other rule(s), so \
                     first-match-wins may shadow it",
                    bucket.len() - 1
                );
                assert_eq!(
                    bucket[0].pattern,
                    accented.to_string(),
                    "{name_type:?}: bucket {accented:?} holds the wrong rule"
                );
                assert_eq!(
                    bucket[0].phonemes.len(),
                    1,
                    "{name_type:?}: {accented:?} should offer exactly one phoneme"
                );
                assert_eq!(
                    bucket[0].phonemes[0].0,
                    plain.to_string(),
                    "{name_type:?}: {accented:?} must produce the phoneme {plain:?}"
                );
            }
        }
    }

    #[test]
    fn no_rule_field_in_the_corpus_carries_a_replacement_character() {
        // U+FFFD in a rule field is always corruption, never data: a
        // pattern spelled `\u{FFFD}` matches no realistic input, and the
        // Rules pass's `OnUnmatched::Skip` then deletes the character the
        // rule existed to encode instead of failing. Enumerates every rule
        // of every embedded file -- the whole point is that this class of
        // damage can never again be present in only a couple of files and
        // go unnoticed.
        const REPLACEMENT: char = '\u{FFFD}';
        let mut rules_checked = 0usize;
        for &(filename, text) in EMBEDDED_FILES {
            for line in meaningful_lines(text) {
                let Some(Line::Rule(raw)) = parse_line(line) else {
                    continue;
                };
                rules_checked += 1;
                for (field_name, field) in [
                    ("pattern", raw.pattern),
                    ("left context", raw.left_context),
                    ("right context", raw.right_context),
                    ("phonetic", raw.phonetic),
                ] {
                    assert!(
                        !field.contains(REPLACEMENT),
                        "U+FFFD in the {field_name} field of {filename}: {line:?}"
                    );
                }
            }
        }
        // A floor, so a walk that silently stops finding rules (a parser
        // change, a renamed file list) fails here instead of passing
        // vacuously. The corpus holds ~4,300 rules across its 127 files.
        assert!(
            rules_checked >= 4_000,
            "only {rules_checked} rules walked -- the corpus scan is not covering the corpus"
        );
    }

    /// Every `(NameType, PassKind, language-file-suffix)` triple that has a
    /// real file behind it, derived from the embedded file list rather than
    /// from a hand-kept list that could drift out of date.
    fn every_rule_table() -> Vec<(NameType, PassKind, &'static str)> {
        let mut out = Vec::new();
        for &(filename, _) in EMBEDDED_FILES {
            let mut parts = filename.splitn(3, '_');
            let (Some(prefix), Some(infix), Some(suffix)) =
                (parts.next(), parts.next(), parts.next())
            else {
                continue;
            };
            let name_type = match prefix {
                "gen" => NameType::Generic,
                "ash" => NameType::Ashkenazi,
                "sep" => NameType::Sephardic,
                _ => continue,
            };
            let pass = match infix {
                "rules" => PassKind::Rules,
                "approx" => PassKind::Refine(RuleType::Approx),
                "exact" => PassKind::Refine(RuleType::Exact),
                _ => continue,
            };
            out.push((name_type, pass, suffix));
        }
        out
    }

    #[test]
    fn every_rule_is_bucketed_under_its_compiled_patterns_first_character() {
        // `by_first_char` is an index, and the pattern a rule is *matched*
        // by is the compiled, unescaped one -- so the key must come from
        // that same string. Keying off the raw, still-escaped field files a
        // `\"` rule under `\` while it only ever matches `"`, which is this
        // migration's signature defect: an index built on the pre-transform
        // spelling and consulted with the post-transform one.
        //
        // Enumerates every bucket of every table the corpus can produce,
        // rather than checking the handful of rules known to use the
        // escape.
        let mut tables_checked = 0usize;
        for (name_type, pass, suffix) in every_rule_table() {
            let compiled = name_type_data(name_type).table(name_type, pass, suffix);
            tables_checked += 1;
            for (&ch, rules) in &compiled.table.by_first_char {
                for rule in rules {
                    assert_eq!(
                        rule.pattern.chars().next().unwrap_or('\u{0}'),
                        ch,
                        "{name_type:?}/{pass:?}/{suffix}: rule {:?} is filed under {ch:?}, which \
                         is not the first character of the pattern it matches by",
                        rule.pattern
                    );
                }
            }
        }
        assert!(
            tables_checked >= 100,
            "only {tables_checked} tables walked -- the table sweep is not covering the corpus"
        );
    }

    /// **Enumeration, not sampling.** Every rule of every table the embedded
    /// corpus can produce, walked for reachability — and the exact count of
    /// dead rules asserted, because it is not zero.
    ///
    /// Within a bucket the *first* matching rule wins, so a rule is
    /// definitely unreachable when an earlier rule in the same bucket has the
    /// same pattern **and** no context conditions at all: that earlier rule
    /// fires at every position this one could. (A rule shadowed only under
    /// *some* contexts is not decidable from the tables alone, and is not
    /// claimed here, so 42 is a lower bound on dead rules, not the total.)
    ///
    /// # What the 42 are, and why they are not repaired here
    ///
    /// **32 of them are one upstream copy-paste error**, in
    /// `ash_approx_common.txt` and `gen_approx_common.txt`. Each file has a
    /// run of nine six-line blocks, one per vowel, mapping `<v>j<v>` to `D`.
    /// The `a`/`A` block is correct; **every later block repeats the `A`
    /// block's last two lines verbatim** instead of using its own vowel:
    ///
    /// ```text
    /// "Aja"  ""  ""  "D"      "oja"  ""  ""  "D"
    /// "AjA"  ""  ""  "D"      "ojA"  ""  ""  "D"
    /// "Ajo"  ""  ""  "D"      "ojo"  ""  ""  "D"
    /// "AjO"  ""  ""  "D"      "ojO"  ""  ""  "D"
    /// "Aju"  ""  ""  "D"      "Aju"  ""  ""  "D"   <-- should be "oju"
    /// "AjU"  ""  ""  "D"      "AjU"  ""  ""  "D"   <-- should be "ojU"
    /// ```
    ///
    /// So `"Aju"` and `"AjU"` each appear **nine** times while `oju`, `Oju`,
    /// `eju`, `Eju`, `iju`, `Iju`, `uju`, `Uju` and their `U` variants appear
    /// **zero** times in either file. The duplicates are harmless in
    /// themselves — they would emit the same phoneme — but the rules they
    /// displaced are simply absent, so those vowel-`j`-vowel sequences are not
    /// collapsed the way the `a` block's are.
    ///
    /// The remaining 10 are single-rule duplicates in the Ashkenazi and
    /// Sephardic `rules` files (`h`, `j`, `ej`, `goltz`, `ű`, `z`).
    ///
    /// **Verbora does not repair this.** The rule corpus is Apache-2.0 data
    /// copied verbatim from Apache Commons Codec, and the only other
    /// transcription of Beider and Morse's original is GPL-3.0 PHP that this
    /// workspace may not read (see `data/beider-morse/NOTICE.md`). Writing the
    /// eight missing rules would mean *inventing* behaviour with no citable
    /// basis, which is exactly what this migration exists to remove. The
    /// defect is therefore recorded, counted and pinned instead: if a future
    /// corpus update fixes it upstream, this assertion fails and the count is
    /// updated deliberately.
    #[test]
    fn the_corpus_dead_rule_count_is_exactly_what_upstream_left_behind() {
        let mut tables = 0usize;
        let mut rules = 0usize;
        let mut shadowed: Vec<String> = Vec::new();

        for (name_type, pass, suffix) in every_rule_table() {
            let compiled = name_type_data(name_type).table(name_type, pass, suffix);
            tables += 1;
            for (first, bucket) in &compiled.table.by_first_char {
                for (i, rule) in bucket.iter().enumerate() {
                    rules += 1;
                    if let Some(earlier) = bucket[..i]
                        .iter()
                        .position(|e| e.pattern == rule.pattern && e.is_unconditional())
                    {
                        shadowed.push(format!(
                            "{name_type:?}/{suffix}/{pass:?} bucket {first:?}: rule {i} \
                             ({:?}) is shadowed by unconditional rule {earlier}",
                            rule.pattern
                        ));
                    }
                }
            }
        }

        assert_eq!(
            shadowed.len(),
            42,
            "the corpus's dead-rule count changed:\n{}",
            shadowed.join("\n")
        );
        // The `Aju`/`AjU` duplication is the bulk of it, and is named
        // explicitly so that a corpus fix cannot quietly rebalance the total.
        let aju = shadowed
            .iter()
            .filter(|line| line.contains("\"Aju\"") || line.contains("\"AjU\""))
            .count();
        assert_eq!(
            aju,
            32,
            "the Aju/AjU copy-paste run changed:\n{}",
            shadowed.join("\n")
        );
        assert!(tables >= 100, "only {tables} rule tables were walked");
        assert!(rules >= 4_500, "only {rules} rules were walked");
    }

    #[test]
    fn literal_quote_rule_is_reachable_in_the_real_rule_table() {
        // `\"` is the one escape the rule DSL defines, and the corpus uses
        // it in four files. `rule.rs` proves in isolation that
        // `Rule::compile` unescapes it; this proves the rule is reachable in
        // the table it actually lives in, which is a separate claim and was
        // the false one.
        const FILES_USING_THE_ESCAPE: [(NameType, &str); 4] = [
            (NameType::Generic, "russian"),
            (NameType::Generic, "any"),
            (NameType::Ashkenazi, "russian"),
            (NameType::Ashkenazi, "any"),
        ];

        for (name_type, suffix) in FILES_USING_THE_ESCAPE {
            // The corpus really does still carry the escape unexpanded at
            // parse time -- otherwise the rest of this test would pass for
            // the wrong reason.
            let mut raw = Vec::new();
            load_rule_lines(
                &format!("{}_rules_{suffix}", name_type.file_prefix()),
                &mut raw,
            );
            assert!(
                raw.iter().any(|r| r.pattern == "\\\""),
                "{name_type:?}/{suffix}: expected a raw `\\\"` pattern in the corpus"
            );

            let data = name_type_data(name_type);
            let compiled = data.table(name_type, PassKind::Rules, suffix);
            assert!(
                !compiled.table.by_first_char.contains_key(&'\\'),
                "{name_type:?}/{suffix}: a rule is filed under the escape character `\\`"
            );
            let bucket =
                compiled.table.by_first_char.get(&'"').unwrap_or_else(|| {
                    panic!("{name_type:?}/{suffix}: no bucket for a literal quote")
                });
            assert!(
                bucket.iter().any(|r| r.pattern == "\""),
                "{name_type:?}/{suffix}: the quote bucket holds no literal-quote rule"
            );

            // And it fires: the rule's phonetic field is empty, so a `"` in
            // the input is consumed and contributes nothing. Read with
            // `OnUnmatched::PassThrough` so that an unreachable rule shows
            // up as the character surviving into the output instead of
            // being indistinguishable from a silent skip.
            let mut builder = PhonemeBuilder::empty(data.all_languages());
            compiled.table.apply_to(
                "\"",
                &mut builder,
                DEFAULT_MAX_PHONEMES,
                OnUnmatched::PassThrough,
            );
            let candidates = builder.into_candidates();
            assert_eq!(candidates.len(), 1);
            assert_eq!(
                candidates[0].text, "",
                "{name_type:?}/{suffix}: the literal-quote rule never fired"
            );
        }
    }
}
