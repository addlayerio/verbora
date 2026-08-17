//! Beider-Morse Phonetic Matching (BMPM) — a Verbora-native extension, not
//! a reference-parity port (the reference has no Beider-Morse implementation).
//!
//! # What this is, and why it exists alongside [`crate::SoundExDM`]
//!
//! [`crate::SoundExDM`] (Daitch-Mokotoff) already covers Slavic/Germanic/
//! Ashkenazi-Jewish surname matching with one fixed rule table. Beider-Morse
//! solves a different, harder problem: the *same* historical family name
//! plausibly has different phonetically-equivalent spellings depending on
//! *which country's* orthographic conventions transcribed it — a name
//! carried from Russia through Poland to Germany accumulates several
//! "correct" spellings, not one. Beider-Morse's rule tables are
//! per-language, and its engine can either target one specific language's
//! conventions or blend the "any"-language fallback rules to hedge across
//! all of them at once.
//!
//! # Provenance and licensing — read before touching `data/beider-morse/`
//!
//! The 127 rule files this module reads (`crates/verbora-phonetics/data/beider-morse/`)
//! are Apache-2.0-licensed data copied from Apache Commons Codec (itself a
//! Java port of Alexander Beider and Stephen P. Morse's original,
//! GPL-3.0-licensed PHP reference implementation) — **not** a copy of the
//! GPL-licensed PHP source itself. See `data/beider-morse/NOTICE.md` for the
//! full provenance chain and why embedding Apache-2.0 data in this
//! MIT-licensed crate is fine. The engine and parser in this module
//! (`rule.rs`, `engine.rs`, this file) are Verbora's own MIT-licensed Rust,
//! written from this module's own understanding of the algorithm — not a
//! transliteration of any other implementation's source code.
//!
//! # Design reference used, not a runtime dependency
//!
//! The [`rphonetic`](https://crates.io/crates/rphonetic) crate (a mature,
//! independently-verified Rust port of the same Commons Codec algorithm)
//! was read closely during design and used as a live cross-checking oracle
//! during development (running the exact same rule files through both
//! implementations and diffing output) — it is not a dependency of this
//! crate, published or otherwise; see `docs/COMPETITIVE_BENCHMARKS.md` for
//! where it *is* a real dependency (the isolated `benchmarks/competitive/`
//! workspace, for an unrelated purpose: benchmarking Verbora's other four
//! phonetic encoders against it).
//!
//! # Output shape — deliberately not [`crate::PhoneticCodes`]
//!
//! Every other encoder in this crate produces exactly one or two codes per
//! word, which is what [`crate::PhoneticCodes`]'s `One`/`Two` shape is
//! built around. Beider-Morse's real output is a *variable-length* set of
//! plausible spellings — bounded by `max_phonemes` (default 20), not by 1
//! or 2 (a name like `"Renault"` alone produces 8 alternatives under
//! Generic/Approx/any). Forcing that into `PhoneticCodes` would either
//! silently truncate real candidates or require widening every other
//! encoder's shape to accommodate a cardinality only this one needs.
//! [`BeiderMorseCode`] is its own type instead; whether/how this composes
//! with [`crate::PhoneticIndex`] is left for a follow-up once this engine's
//! correctness is established across its full language matrix — not
//! designed speculatively ahead of that.

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

/// Which family-naming convention's rule tables to apply. See this module's
/// own doc comment for why this is a real choice, not a formality: it picks
/// which language-specific rule files exist to draw from at all (Generic:
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
    /// conventionally split around rather than encoded as one run — matches
    /// every reference implementation's own fixed list per name type (not
    /// derived from the rule corpus itself, since these are a naming-
    /// convention fact, not a phonetic one). Note `"de la"` is itself a
    /// two-word entry: the prefix check below matches it as one literal
    /// string against the *unsplit* input, before word-splitting happens.
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

/// How wide a net the final refinement pass casts. See this module's own
/// doc comment section on terminology — these are the two real, documented
/// engine parameters (not the "genealogical/phonetic" framing floated
/// early in this feature's own design discussion and dropped once it
/// didn't check out against any primary source).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RuleType {
    /// Casts the widest net across plausible historical/cross-language
    /// spelling drift — the default in every reference implementation
    /// surveyed, and the mode that produces Beider-Morse's actual
    /// selling point over a single-language algorithm.
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
        if let Some(t) = self.tables.read().unwrap().get(&key) {
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
            let ch = raw_rule.pattern.chars().next().unwrap_or('\u{0}');
            by_first_char.entry(ch).or_default().push(compiled);
        }
        let compiled = std::sync::Arc::new(CompiledTable {
            table: RuleTable { by_first_char },
        });
        self.tables
            .write()
            .unwrap()
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
/// [`BeiderMorse::encode`] produced, deduplicated by text. See this
/// module's own doc comment for why this is not [`crate::PhoneticCodes`].
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
    /// (literal parens/pipes/hyphens are part of that one string, matching
    /// every reference implementation's own single-`String` return shape
    /// for these cases), not independent candidates — treat it as one
    /// opaque compound key, not a set to iterate. When `false` (the common
    /// case), every element of `spellings` is an independent candidate as
    /// usual.
    pub compound: bool,
}

/// The Beider-Morse encoder for one [`NameType`] and [`RuleType`]
/// combination. Cheap to construct (holds only a few small fields; the
/// actual rule tables are cached process-wide, not per-instance — see
/// [`name_type_data`]).
#[derive(Debug, Clone, Copy)]
pub struct BeiderMorse {
    name_type: NameType,
    rule_type: RuleType,
    max_phonemes: usize,
    concat: bool,
}

const DEFAULT_MAX_PHONEMES: usize = 20;

impl BeiderMorse {
    /// A Beider-Morse encoder for `name_type`/`rule_type`, with the
    /// reference implementations' own defaults: candidate cap 20, `concat`
    /// on (see [`Self::with_concat`] — every reference implementation's own
    /// builder defaults to `true` despite what its doc comment claims;
    /// confirmed against the actual default-constructor source, not the
    /// comment).
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
    /// spelled under from the spelling itself (see [`lang::LangGuesser`]) —
    /// matches every reference implementation's own default behavior. A
    /// confident single-language guess (e.g. `"Renault"` → French) loads
    /// that language's own rule file and starts every candidate phoneme
    /// pre-filtered to it; an ambiguous guess falls back to the `"any"` file
    /// with the (possibly still narrowed) guessed set as the starting
    /// languages, same as [`Self::encode_language`] never running.
    ///
    /// Also handles what every reference implementation calls the
    /// "genealogical" surname shapes: a leading `d'` (`"d'Angelo"`) or a
    /// [`NameType`]-specific name prefix (`"van Gogh"`, `"de la Cruz"` —
    /// Generic only) splits the name into `(without-the-prefix)-(with-the-
    /// prefix-fused-on)`, each re-encoded independently; a name with more
    /// than one word (and [`Self::with_concat`]`(false)`) encodes each word on
    /// its own and hyphen-joins the results, rather than treating the whole
    /// string as one phonetic run. In both cases the returned
    /// [`BeiderMorseCode::compound`] is `true` and `spellings` holds exactly
    /// one already-composed string (matching every reference
    /// implementation's own single-`String` return shape for these cases),
    /// not independent candidates — check `compound` before iterating
    /// `spellings` as a candidate set; see that field's own doc comment.
    #[must_use]
    pub fn encode(&self, word: &str) -> BeiderMorseCode {
        self.encode_top(word, None)
    }

    /// Encodes `word` restricted to the single named language (e.g.
    /// `"russian"`, `"english"`) — a smaller candidate set than
    /// [`Self::encode`], since only that language's own rule file (plus the
    /// shared `common` rules) is consulted, and every phoneme is
    /// pre-filtered to just that language throughout. Prefix- and
    /// multi-word-splitting (see [`Self::encode`]) still apply; matching
    /// every reference implementation, the *split-off parts* are always
    /// re-guessed from scratch rather than inheriting `language` — only the
    /// base case (a single already-split word) actually restricts to it.
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
/// (not the original word) — the refinement pass operates on what the
/// Rules pass already produced, per every reference implementation's own
/// two-stage shape (see `engine.rs`'s own doc comment).
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
        // "Renault" guesses to a singleton French match and its 8-candidate
        // output was diffed byte-for-byte against a live `rphonetic` oracle
        // (built from the identical rule corpus) during development -- see
        // this module's own doc comment for why that oracle isn't a
        // committed dependency, so it isn't re-asserted verbatim here.
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
}
