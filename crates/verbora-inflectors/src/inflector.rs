//! The shared engine: `TenseInflector.ize` and the four inflectors built on it.
//!
//! # The algorithm
//!
//! ```text
//! ize (token, formSet, customForms) {
//!   const restoreCase = this.restoreCase(token)
//!   return restoreCase(this.izeRegExps(token, customForms) || this.izeAbiguous(token) ||
//!         this.izeRegulars(token, formSet) || this.izeRegExps(token, formSet.regularForms) ||
//!         token)
//! }
//! ```
//!
//! Four stages, highest precedence first: caller-added rules, the invariant
//! ("ambiguous") list, the irregular table, the built-in rules — then the token
//! unchanged. The case-restoration function is computed from the *original*
//! token and applied to whichever stage won.
//!
//! # The trap: `||` treats `""` as no match
//!
//! This is the single most consequential quirk in the module. A stage that
//! genuinely matched but rewrote the token to the empty string is discarded by
//! `||`, and the chain continues to the next stage — ultimately returning the
//! token unchanged:
//!
//! | Call | Rule that fired | the reference |
//! |---|---|---|
//! | `PresentVerbInflector::pluralize("Es")` | `/e?s$/i → ''` | `"Es"` |
//! | `PresentVerbInflector::pluralize("s")` | `/e?s$/i → ''` | `"s"` |
//! | `NounInflectorFr::singularize("S")` | `/(.*)s$/i → '$1'` | `"S"` |
//! | `pluralize("cat")` after `add_plural(^cat$ → "")` | the custom rule | `"cats"` |
//!
//! A Rust port that models each stage as `Option<String>` and returns `Some("")`
//! emits the empty string for all four. Every stage here is therefore filtered
//! with `!s.is_empty()`, and [`Rule::apply`] deliberately keeps "matched" and
//! "produced something" as separate signals so the filter has something to act
//! on. `izeRegulars` carries the same trap — `if (formSet.irregularForms[token])`
//! is a truthiness test, not a presence test — and is filtered identically.
//!
//! # Shared, immutable rule tables
//!
//! The reference rebuilds both `FormSet`s, the ambiguous array and every
//! `RegExp` inside each constructor, so `new NounInflectorFr()` reconstructs a
//! 744-entry array every time. Here the built-in tables are compiled once per
//! process behind a `LazyLock` and shared by every instance; only the
//! caller-added rules are per-instance. Construction is consequently a couple of
//! pointer copies, and instances remain fully independent — verified against the
//! reference, whose custom forms are also per-instance.

use std::borrow::Cow;
use std::fmt;
use std::sync::LazyLock;

use crate::case::restore_case;
use crate::data;
use crate::rules::{FormSet, Forms, Rule};

/// The error every inflector returns for the empty token.
///
/// The reference throws `TypeError: Cannot read properties of undefined (reading
/// 'toUpperCase')` from `restoreCase`, which reads `token[0]` without checking.
/// Returning `""` instead would be a silent divergence — `""` is not what the
/// reference produces, because the reference produces nothing at all — so the
/// throw is surfaced as a `Result`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct EmptyToken;

impl fmt::Display for EmptyToken {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("cannot inflect the empty token")
    }
}

impl std::error::Error for EmptyToken {}

/// State shared by all four inflectors: static tables plus per-instance
/// additions.
pub(crate) struct Core {
    /// `this.ambiguous`, sorted for binary search.
    ambiguous: &'static [&'static str],
    /// The built-in rules for both directions.
    forms: &'static Forms,
    /// `this.customPluralForms`, in insertion order — earliest wins.
    custom_plural: Vec<Rule>,
    /// `this.customSingularForms`.
    custom_singular: Vec<Rule>,
    /// Runtime `addIrregular` writes, shadowing the static table.
    added_plural_irregular: Vec<(String, String)>,
    /// The reverse direction of the same writes.
    added_singular_irregular: Vec<(String, String)>,
}

impl Core {
    fn new(ambiguous: &'static [&'static str], forms: &'static Forms) -> Self {
        Self {
            ambiguous,
            forms,
            custom_plural: Vec::new(),
            custom_singular: Vec::new(),
            added_plural_irregular: Vec::new(),
            added_singular_irregular: Vec::new(),
        }
    }

    fn pluralize_into(&self, token: &str, out: &mut String) -> Result<(), EmptyToken> {
        self.ize_into(
            token,
            &self.forms.plural,
            &self.custom_plural,
            &self.added_plural_irregular,
            out,
        )
    }

    fn singularize_into(&self, token: &str, out: &mut String) -> Result<(), EmptyToken> {
        self.ize_into(
            token,
            &self.forms.singular,
            &self.custom_singular,
            &self.added_singular_irregular,
            out,
        )
    }

    /// `TenseInflector.ize`.
    fn ize_into(
        &self,
        token: &str,
        form_set: &FormSet,
        custom: &[Rule],
        added: &[(String, String)],
        out: &mut String,
    ) -> Result<(), EmptyToken> {
        let mode = restore_case(token).ok_or(EmptyToken)?;

        // `izeAbiguous` and `izeRegulars` both lowercase the token; the
        // reference folds it three times per call, this folds once — and not at
        // all for the already-lowercase ASCII input that dominates real corpora.
        let lower = lowercase_units(token);

        let winner: Option<Cow<'_, str>> = first_match(custom, token)
            .filter(|s| !s.is_empty())
            .map(Cow::Owned)
            .or_else(|| {
                // `izeAbiguous` returns the LOWERCASED token, which
                // `restoreCase` then re-cases: `pluralize("dEer")` is `"deer"`.
                self.ambiguous
                    .binary_search(&lower.as_ref())
                    .is_ok()
                    .then(|| Cow::Borrowed(lower.as_ref()))
                    .filter(|s: &Cow<'_, str>| !s.is_empty())
            })
            .or_else(|| {
                irregular(added, form_set, &lower)
                    .map(Cow::Borrowed)
                    .filter(|s: &Cow<'_, str>| !s.is_empty())
            })
            .or_else(|| {
                first_match(&form_set.regular, token)
                    .filter(|s| !s.is_empty())
                    .map(Cow::Owned)
            });

        mode.apply_into(winner.as_deref().unwrap_or(token), out);
        Ok(())
    }

    /// `addIrregular`, which is `addForm` with the two tables wired up.
    ///
    /// Both arguments are lowercased, then written in *both* directions —
    /// `pluralTable[singular] = plural` and `singularTable[plural] = singular` —
    /// with plain overwrite. Last write wins, which is why
    /// `PresentVerbInflector`, registering `('am','are')` then `('is','are')`,
    /// singularises `are` to `is` rather than `am`.
    fn add_irregular(&mut self, singular: &str, plural: &str) {
        let singular = singular.to_lowercase();
        let plural = plural.to_lowercase();
        put(&mut self.added_plural_irregular, &singular, &plural);
        put(&mut self.added_singular_irregular, &plural, &singular);
    }
}

/// The first rule that matches, whether or not its result is usable.
fn first_match(rules: &[Rule], token: &str) -> Option<String> {
    rules.iter().find_map(|rule| rule.apply(token))
}

/// `formSet.irregularForms[token]`, with runtime additions shadowing the
/// built-in table — the reference writes both into one object, so a later
/// `addIrregular` for an existing key simply replaces it.
fn irregular<'a>(
    added: &'a [(String, String)],
    form_set: &'a FormSet,
    lower: &str,
) -> Option<&'a str> {
    if let Some((_, v)) = added.iter().find(|(k, _)| k == lower) {
        return Some(v);
    }
    form_set.irregular(lower)
}

/// Inserts or overwrites, mirroring the reference property assignment.
fn put(map: &mut Vec<(String, String)>, key: &str, value: &str) {
    if let Some((_, slot)) = map.iter_mut().find(|(k, _)| k == key) {
        slot.clear();
        slot.push_str(value);
    } else {
        map.push((key.to_owned(), value.to_owned()));
    }
}

/// `String.prototype.toLowerCase`, borrowing when nothing would change.
fn lowercase_units(s: &str) -> Cow<'_, str> {
    if s.is_ascii() {
        if s.bytes().any(|b| b.is_ascii_uppercase()) {
            Cow::Owned(s.to_ascii_lowercase())
        } else {
            Cow::Borrowed(s)
        }
    } else {
        Cow::Owned(s.to_lowercase())
    }
}

// ---------------------------------------------------------------------------
// The public inflectors
// ---------------------------------------------------------------------------

/// The behaviour every singular/plural inflector shares — `TenseInflector` in
/// The reference, where it is the base class of all four.
///
/// Implemented by [`NounInflector`], [`NounInflectorFr`], [`NounInflectorJa`]
/// and [`PresentVerbInflector`]. Each also carries the same methods inherently,
/// so the trait is needed only for code that is generic over inflectors.
pub trait SingularPluralInflector {
    /// Inflects `token` towards the plural.
    ///
    /// # Errors
    ///
    /// [`EmptyToken`] for `""`, where the reference throws.
    fn pluralize(&self, token: &str) -> Result<String, EmptyToken>;

    /// Inflects `token` towards the singular.
    ///
    /// # Errors
    ///
    /// [`EmptyToken`] for `""`, where the reference throws.
    fn singularize(&self, token: &str) -> Result<String, EmptyToken>;

    /// [`Self::pluralize`], appending to a caller-owned buffer.
    ///
    /// # Errors
    ///
    /// [`EmptyToken`] for `""`; `out` is left untouched in that case.
    fn pluralize_into(&self, token: &str, out: &mut String) -> Result<(), EmptyToken>;

    /// [`Self::singularize`], appending to a caller-owned buffer.
    ///
    /// # Errors
    ///
    /// [`EmptyToken`] for `""`; `out` is left untouched in that case.
    fn singularize_into(&self, token: &str, out: &mut String) -> Result<(), EmptyToken>;

    /// Adds a pluralisation rule, consulted before every built-in table.
    fn add_plural(&mut self, rule: Rule);

    /// Adds a singularisation rule, consulted before every built-in table.
    fn add_singular(&mut self, rule: Rule);

    /// Registers an irregular pair in both directions.
    fn add_irregular(&mut self, singular: &str, plural: &str);
}

/// Generates one inflector: the struct, its inherent API, and the trait impl.
///
/// The four differ only in which static tables they point at, so writing the
/// delegation four times would be four chances to get one of them wrong.
macro_rules! inflector {
    (
        $(#[$meta:meta])*
        $name:ident, $forms:ident, $ambiguous:path, $plural_regular:path,
        $plural_irregular:path, $singular_regular:path, $singular_irregular:path
    ) => {
        static $forms: LazyLock<Forms> = LazyLock::new(|| {
            Forms::build(
                &$plural_regular,
                &$plural_irregular,
                &$singular_regular,
                &$singular_irregular,
            )
        });

        $(#[$meta])*
        pub struct $name {
            core: Core,
        }

        impl $name {
            /// Creates an inflector with the reference's built-in rules.
            ///
            /// Cheap: the rule tables are compiled once per process and shared,
            /// so this allocates nothing until a rule is added.
            #[must_use]
            pub fn new() -> Self {
                Self {
                    core: Core::new(&$ambiguous, &$forms),
                }
            }

            /// Inflects `token` towards the plural.
            ///
            /// # Errors
            ///
            /// [`EmptyToken`] for `""`, where the reference throws `TypeError`.
            pub fn pluralize(&self, token: &str) -> Result<String, EmptyToken> {
                let mut out = String::with_capacity(token.len() + 4);
                self.core.pluralize_into(token, &mut out)?;
                Ok(out)
            }

            /// Inflects `token` towards the singular.
            ///
            /// # Errors
            ///
            /// [`EmptyToken`] for `""`, where the reference throws `TypeError`.
            pub fn singularize(&self, token: &str) -> Result<String, EmptyToken> {
                let mut out = String::with_capacity(token.len() + 4);
                self.core.singularize_into(token, &mut out)?;
                Ok(out)
            }

            /// [`Self::pluralize`], appending to `out` instead of allocating.
            ///
            /// `out` is not cleared, so a batch loop can reuse one buffer's
            /// capacity across a whole corpus.
            ///
            /// # Errors
            ///
            /// [`EmptyToken`] for `""`; `out` is left untouched in that case.
            pub fn pluralize_into(
                &self,
                token: &str,
                out: &mut String,
            ) -> Result<(), EmptyToken> {
                self.core.pluralize_into(token, out)
            }

            /// [`Self::singularize`], appending to `out` instead of allocating.
            ///
            /// # Errors
            ///
            /// [`EmptyToken`] for `""`; `out` is left untouched in that case.
            pub fn singularize_into(
                &self,
                token: &str,
                out: &mut String,
            ) -> Result<(), EmptyToken> {
                self.core.singularize_into(token, out)
            }

            /// `addPlural`: appends a rule consulted *before* the ambiguous list
            /// and the irregular table.
            ///
            /// Rules are matched in insertion order, so an earlier addition wins
            /// over a later one. The effect is per-instance.
            pub fn add_plural(&mut self, rule: Rule) {
                self.core.custom_plural.push(rule);
            }

            /// `addSingular`: the singularisation counterpart of
            /// [`Self::add_plural`].
            pub fn add_singular(&mut self, rule: Rule) {
                self.core.custom_singular.push(rule);
            }

            /// `addIrregular`: registers `singular`/`plural` in both directions.
            ///
            /// Both arguments are lowercased first, and each direction is a
            /// plain overwrite, so re-registering a plural replaces the previous
            /// singular for it.
            pub fn add_irregular(&mut self, singular: &str, plural: &str) {
                self.core.add_irregular(singular, plural);
            }
        }

        impl Default for $name {
            fn default() -> Self {
                Self::new()
            }
        }

        impl fmt::Debug for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.debug_struct(stringify!($name))
                    .field("custom_plural_rules", &self.core.custom_plural.len())
                    .field("custom_singular_rules", &self.core.custom_singular.len())
                    .field("added_irregulars", &self.core.added_plural_irregular.len())
                    .finish()
            }
        }

        impl SingularPluralInflector for $name {
            fn pluralize(&self, token: &str) -> Result<String, EmptyToken> {
                Self::pluralize(self, token)
            }
            fn singularize(&self, token: &str) -> Result<String, EmptyToken> {
                Self::singularize(self, token)
            }
            fn pluralize_into(&self, token: &str, out: &mut String) -> Result<(), EmptyToken> {
                Self::pluralize_into(self, token, out)
            }
            fn singularize_into(
                &self,
                token: &str,
                out: &mut String,
            ) -> Result<(), EmptyToken> {
                Self::singularize_into(self, token, out)
            }
            fn add_plural(&mut self, rule: Rule) {
                Self::add_plural(self, rule);
            }
            fn add_singular(&mut self, rule: Rule) {
                Self::add_singular(self, rule);
            }
            fn add_irregular(&mut self, singular: &str, plural: &str) {
                Self::add_irregular(self, singular, plural);
            }
        }
    };
}

inflector! {
    /// English noun inflector — the reference `noun_inflector`.
    ///
    /// ```
    /// use verbora_inflectors::NounInflector;
    ///
    /// let inflector = NounInflector::new();
    /// assert_eq!(inflector.pluralize("radius").unwrap(), "radii");
    /// assert_eq!(inflector.singularize("children").unwrap(), "child");
    /// // The invariant list wins over the regular rules, and the original
    /// // capitalisation is restored afterwards.
    /// assert_eq!(inflector.pluralize("DEER").unwrap(), "DEER");
    /// // `human` is excluded from the -man rule by a negative lookahead.
    /// assert_eq!(inflector.pluralize("human").unwrap(), "humans");
    /// assert_eq!(inflector.pluralize("workman").unwrap(), "workmen");
    /// ```
    NounInflector, EN_FORMS, data::en::AMBIGUOUS, data::en::PLURAL_REGULAR,
    data::en::PLURAL_IRREGULAR, data::en::SINGULAR_REGULAR, data::en::SINGULAR_IRREGULAR
}

inflector! {
    /// French noun inflector — the reference `noun_inflector`.
    ///
    /// ```
    /// use verbora_inflectors::NounInflectorFr;
    ///
    /// let inflector = NounInflectorFr::new();
    /// assert_eq!(inflector.pluralize("cheval").unwrap(), "chevaux");
    /// assert_eq!(inflector.pluralize("œil").unwrap(), "yeux");
    /// assert_eq!(inflector.singularize("bijoux").unwrap(), "bijou");
    /// // 744 invariant nouns, mostly ending in -s, -x or -z.
    /// assert_eq!(inflector.pluralize("rhinocéros").unwrap(), "rhinocéros");
    /// ```
    NounInflectorFr, FR_FORMS, data::fr::AMBIGUOUS, data::fr::PLURAL_REGULAR,
    data::fr::PLURAL_IRREGULAR, data::fr::SINGULAR_REGULAR, data::fr::SINGULAR_IRREGULAR
}

inflector! {
    /// Japanese noun inflector — the reference `noun_inflector`.
    ///
    /// Japanese does not normally mark number, so pluralisation appends
    /// `たち` to anything and singularisation strips a plural suffix unless the
    /// stem is on an exception list.
    ///
    /// ```
    /// use verbora_inflectors::NounInflectorJa;
    ///
    /// let inflector = NounInflectorJa::new();
    /// assert_eq!(inflector.pluralize("私").unwrap(), "私たち");
    /// assert_eq!(inflector.singularize("人たち").unwrap(), "人");
    /// // `かたち` ("shape") only looks like a plural.
    /// assert_eq!(inflector.singularize("かたち").unwrap(), "かたち");
    /// ```
    NounInflectorJa, JA_FORMS, data::ja::AMBIGUOUS, data::ja::PLURAL_REGULAR,
    data::ja::PLURAL_IRREGULAR, data::ja::SINGULAR_REGULAR, data::ja::SINGULAR_IRREGULAR
}

inflector! {
    /// Present-tense verb inflector —
    /// The reference `present_verb_inflector`.
    ///
    /// The names are inverted relative to nouns: `singularize` produces the
    /// third-person singular and `pluralize` produces the base form.
    ///
    /// ```
    /// use verbora_inflectors::PresentVerbInflector;
    ///
    /// let inflector = PresentVerbInflector::new();
    /// assert_eq!(inflector.singularize("go").unwrap(), "goes");
    /// assert_eq!(inflector.pluralize("catches").unwrap(), "catch");
    /// // The irregular table is asymmetric: ('am','are') then ('is','are')
    /// // leaves `are` mapping back to `is`.
    /// assert_eq!(inflector.singularize("are").unwrap(), "is");
    /// ```
    PresentVerbInflector, VERB_FORMS, data::verb::AMBIGUOUS, data::verb::PLURAL_REGULAR,
    data::verb::PLURAL_IRREGULAR, data::verb::SINGULAR_REGULAR, data::verb::SINGULAR_IRREGULAR
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_string_is_rejected_by_every_inflector() {
        assert_eq!(NounInflector::new().pluralize(""), Err(EmptyToken));
        assert_eq!(NounInflector::new().singularize(""), Err(EmptyToken));
        assert_eq!(NounInflectorFr::new().pluralize(""), Err(EmptyToken));
        assert_eq!(NounInflectorJa::new().singularize(""), Err(EmptyToken));
        assert_eq!(PresentVerbInflector::new().pluralize(""), Err(EmptyToken));
    }

    #[test]
    fn empty_rule_output_falls_through_to_the_unchanged_token() {
        let verbs = PresentVerbInflector::new();
        // `/e?s$/i -> ''` matches, produces "", and is discarded.
        assert_eq!(verbs.pluralize("Es").unwrap(), "Es");
        assert_eq!(verbs.pluralize("es").unwrap(), "es");
        assert_eq!(verbs.pluralize("ES").unwrap(), "ES");
        assert_eq!(verbs.pluralize("s").unwrap(), "s");
        assert_eq!(NounInflectorFr::new().singularize("S").unwrap(), "S");

        // The same applies to a caller-added rule.
        let mut nouns = NounInflector::new();
        nouns.add_plural(Rule::from_pattern("^cat$", true, "").unwrap());
        assert_eq!(nouns.pluralize("cat").unwrap(), "cats");
    }

    #[test]
    fn custom_rules_outrank_the_invariant_and_irregular_tables() {
        let mut nouns = NounInflector::new();
        nouns.add_plural(Rule::from_pattern("deer", true, "deerz").unwrap());
        nouns.add_singular(Rule::from_pattern("child", true, "childx").unwrap());
        assert_eq!(nouns.pluralize("deer").unwrap(), "deerz");
        assert_eq!(nouns.singularize("children").unwrap(), "childxren");

        // Strictly per-instance.
        let fresh = NounInflector::new();
        assert_eq!(fresh.pluralize("deer").unwrap(), "deer");
        assert_eq!(fresh.singularize("children").unwrap(), "child");
    }

    #[test]
    fn earlier_custom_rules_win() {
        let mut nouns = NounInflector::new();
        nouns.add_plural(Rule::from_pattern("o", true, "FIRST").unwrap());
        nouns.add_plural(Rule::from_pattern("o", true, "SECOND").unwrap());
        // Lowercased on the way out: "dog" selects `lowercaseify`, which is
        // applied to whatever the rules produced, not to the original token.
        assert_eq!(nouns.pluralize("dog").unwrap(), "dfirstg");
    }

    #[test]
    fn runtime_irregulars_shadow_the_static_table_and_lowercase_both_arguments() {
        let mut nouns = NounInflector::new();
        nouns.add_irregular("MOUSE", "MOUSES");
        assert_eq!(nouns.pluralize("mouse").unwrap(), "mouses");
        assert_eq!(nouns.singularize("mouses").unwrap(), "mouse");
        // The built-in reverse mapping is untouched.
        assert_eq!(nouns.singularize("mice").unwrap(), "mouse");

        // Second write to the same plural key replaces the singular.
        nouns.add_irregular("rat", "mouses");
        assert_eq!(nouns.singularize("mouses").unwrap(), "rat");
    }

    #[test]
    fn buffer_api_appends_and_leaves_the_buffer_alone_on_error() {
        let nouns = NounInflector::new();
        let mut buf = String::from("<");
        nouns.pluralize_into("hacker", &mut buf).unwrap();
        assert_eq!(buf, "<hackers");
        assert_eq!(nouns.pluralize_into("", &mut buf), Err(EmptyToken));
        assert_eq!(buf, "<hackers");
    }

    #[test]
    fn generic_code_can_use_the_trait() {
        fn plural_of<I: SingularPluralInflector>(i: &I, t: &str) -> String {
            i.pluralize(t).unwrap()
        }
        assert_eq!(plural_of(&NounInflector::new(), "party"), "parties");
        assert_eq!(plural_of(&NounInflectorFr::new(), "cheval"), "chevaux");
    }
}
