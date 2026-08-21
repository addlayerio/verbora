use std::borrow::Cow;
use std::fmt;
use std::sync::LazyLock;

use crate::case::CaseMode;
use crate::data::{self, RawRule};
use crate::rule::{Applied, Rule};

/// One direction of one language: the lexical lists plus the ordered rules.
pub(crate) struct FormSet {
    /// Words this direction leaves alone, in ascending byte order.
    invariant: &'static [&'static str],
    /// Forms no rule derives, keyed by the input word, in ascending byte order.
    irregular: &'static [(&'static str, &'static str)],
    /// The ordered rule list. First match wins.
    regular: Vec<Rule>,
}

impl FormSet {
    fn build(
        invariant: &'static [&'static str],
        irregular: &'static [(&'static str, &'static str)],
        raw: &'static [RawRule],
    ) -> Self {
        Self {
            invariant,
            irregular,
            regular: raw.iter().map(compile_raw).collect(),
        }
    }

    fn is_invariant(&self, lower: &str) -> bool {
        self.invariant.binary_search(&lower).is_ok()
    }

    fn irregular(&self, lower: &str) -> Option<&'static str> {
        self.irregular
            .binary_search_by_key(&lower, |(k, _)| k)
            .ok()
            .and_then(|i| self.irregular.get(i))
            .map(|(_, v)| *v)
    }
}

/// Compiles one table entry.
///
/// The `expect` is not reachable through any public entry point: the table
/// walker in this module's tests compiles every rule of every table, so a
/// malformed pattern fails the test suite rather than a caller's program.
fn compile_raw(raw: &RawRule) -> Rule {
    let built = match raw.replacement {
        Some(replacement) => Rule::new(raw.pattern, replacement),
        None => Rule::keep(raw.pattern),
    };
    built.unwrap_or_else(|e| panic!("built-in rule `{}` is malformed: {e}", raw.pattern))
}

/// Both directions of one language.
pub(crate) struct Forms {
    plural: FormSet,
    singular: FormSet,
}

/// State shared by every inflector: the static tables, plus this instance's own
/// additions.
pub(crate) struct Core {
    forms: &'static Forms,
    custom_plural: Vec<Rule>,
    custom_singular: Vec<Rule>,
    added_plural: Vec<(String, String)>,
    added_singular: Vec<(String, String)>,
}

impl Core {
    fn new(forms: &'static Forms) -> Self {
        Self {
            forms,
            custom_plural: Vec::new(),
            custom_singular: Vec::new(),
            added_plural: Vec::new(),
            added_singular: Vec::new(),
        }
    }

    fn pluralize_into(&self, token: &str, out: &mut String) {
        inflect_into(
            token,
            &self.forms.plural,
            &self.custom_plural,
            &self.added_plural,
            out,
        );
    }

    fn singularize_into(&self, token: &str, out: &mut String) {
        inflect_into(
            token,
            &self.forms.singular,
            &self.custom_singular,
            &self.added_singular,
            out,
        );
    }

    fn add_irregular(&mut self, singular: &str, plural: &str) {
        let singular = singular.to_lowercase();
        let plural = plural.to_lowercase();
        put(&mut self.added_plural, &singular, &plural);
        put(&mut self.added_singular, &plural, &singular);
    }
}

/// The four-stage pipeline. See the crate documentation for the contract.
fn inflect_into(
    token: &str,
    forms: &FormSet,
    custom: &[Rule],
    added: &[(String, String)],
    out: &mut String,
) {
    // The empty token has no inflected form and is returned as it is.
    if token.is_empty() {
        return;
    }

    let mode = CaseMode::of(token);

    // Stage 1: rules the caller added, in the order they were added.
    for rule in custom {
        if let Some(applied) = rule.apply_inner(token) {
            emit(applied, token, mode, out);
            return;
        }
    }

    // The lexical lists are keyed by the lowercase form. Folding once here is
    // the only fold in the call, and ASCII lowercase input skips it entirely.
    let lower = lowercase(token);

    // Stage 2: words this direction leaves alone.
    if forms.is_invariant(&lower) {
        out.push_str(token);
        return;
    }

    // Stage 3: irregular forms, with this instance's additions shadowing the
    // built-in table.
    if let Some(form) = irregular(added, forms, &lower) {
        mode.apply_into(form, out);
        return;
    }

    // Stage 4: the ordered rule list.
    for rule in &forms.regular {
        if let Some(applied) = rule.apply_inner(token) {
            emit(applied, token, mode, out);
            return;
        }
    }

    // Nothing claimed the token.
    out.push_str(token);
}

/// Writes the winning stage's result, restoring case only when the stage
/// actually produced a different form.
fn emit(applied: Applied, token: &str, mode: CaseMode, out: &mut String) {
    match applied {
        Applied::Unchanged => out.push_str(token),
        Applied::Form(form) => mode.apply_into(&form, out),
    }
}

/// Instance additions first, then the built-in table. An addition for a key the
/// table already has replaces it.
fn irregular<'a>(added: &'a [(String, String)], forms: &FormSet, lower: &str) -> Option<&'a str> {
    if let Some((_, value)) = added.iter().find(|(k, _)| k == lower) {
        return (!value.is_empty()).then_some(value.as_str());
    }
    forms.irregular(lower)
}

/// Inserts or overwrites.
fn put(map: &mut Vec<(String, String)>, key: &str, value: &str) {
    if let Some((_, slot)) = map.iter_mut().find(|(k, _)| k == key) {
        slot.clear();
        slot.push_str(value);
    } else {
        map.push((key.to_owned(), value.to_owned()));
    }
}

/// Full Unicode lowercase, borrowing when nothing would change.
fn lowercase(s: &str) -> Cow<'_, str> {
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

/// The behaviour every singular/plural inflector shares.
///
/// Implemented by [`NounInflector`], [`NounInflectorFr`], [`NounInflectorJa`]
/// and [`PresentVerbInflector`]. Each type also carries the same methods
/// inherently, so the trait is only needed by code that is generic over
/// inflectors.
///
/// Every method is total: there is no input for which an inflector fails or
/// panics.
pub trait SingularPluralInflector {
    /// Inflects `token` towards the plural, returning a new [`String`].
    fn pluralize(&self, token: &str) -> String;

    /// Inflects `token` towards the singular, returning a new [`String`].
    fn singularize(&self, token: &str) -> String;

    /// [`Self::pluralize`], **appending** to a caller-owned buffer.
    fn pluralize_into(&self, token: &str, out: &mut String);

    /// [`Self::singularize`], **appending** to a caller-owned buffer.
    fn singularize_into(&self, token: &str, out: &mut String);

    /// Adds a pluralisation rule, consulted before every built-in table.
    fn add_plural(&mut self, rule: Rule);

    /// Adds a singularisation rule, consulted before every built-in table.
    fn add_singular(&mut self, rule: Rule);

    /// Registers an irregular pair in both directions.
    fn add_irregular(&mut self, singular: &str, plural: &str);
}

/// Generates one inflector: the struct, its inherent API and the trait impl.
///
/// The four differ only in which static tables they point at, so writing the
/// delegation out four times would be four chances to get one of them wrong.
macro_rules! inflector {
    (
        $(#[$meta:meta])*
        $name:ident, $forms:ident, $lang:ident,
        $plural_invariant:ident, $singular_invariant:ident
    ) => {
        static $forms: LazyLock<Forms> = LazyLock::new(|| Forms {
            plural: FormSet::build(
                data::$lang::$plural_invariant,
                data::$lang::PLURAL_IRREGULAR,
                data::$lang::PLURAL_REGULAR,
            ),
            singular: FormSet::build(
                data::$lang::$singular_invariant,
                data::$lang::SINGULAR_IRREGULAR,
                data::$lang::SINGULAR_REGULAR,
            ),
        });

        $(#[$meta])*
        pub struct $name {
            core: Core,
        }

        impl $name {
            /// Creates an inflector carrying the built-in rules.
            ///
            /// Cheap: the rule tables are compiled once per process and shared
            /// by every instance, so this allocates nothing until a rule is
            /// added. Instances remain independent — a rule added to one is
            /// invisible to the others.
            #[must_use]
            pub fn new() -> Self {
                Self { core: Core::new(&$forms) }
            }

            /// Inflects `token` towards the plural.
            ///
            /// Total: every input, including `""`, produces a `String`. See
            /// [Choosing the right API](crate#choosing-the-right-api) for when
            /// to prefer [`Self::pluralize_into`].
            #[must_use]
            pub fn pluralize(&self, token: &str) -> String {
                let mut out = String::with_capacity(token.len() + 4);
                self.core.pluralize_into(token, &mut out);
                out
            }

            /// Inflects `token` towards the singular.
            ///
            /// Total: every input, including `""`, produces a `String`.
            #[must_use]
            pub fn singularize(&self, token: &str) -> String {
                let mut out = String::with_capacity(token.len() + 4);
                self.core.singularize_into(token, &mut out);
                out
            }

            /// [`Self::pluralize`], **appending** to `out` instead of
            /// allocating.
            ///
            /// `out` is never cleared, so a batch loop must clear it itself —
            /// forgetting to is a silent correctness bug, which is the price of
            /// the saved allocation. See
            /// [Choosing the right API](crate#choosing-the-right-api).
            pub fn pluralize_into(&self, token: &str, out: &mut String) {
                self.core.pluralize_into(token, out);
            }

            /// [`Self::singularize`], **appending** to `out` instead of
            /// allocating.
            pub fn singularize_into(&self, token: &str, out: &mut String) {
                self.core.singularize_into(token, out);
            }

            /// Adds a pluralisation rule, consulted before every built-in
            /// table.
            ///
            /// Rules are matched in the order they were added, so an earlier
            /// addition wins over a later one. The effect is per-instance.
            pub fn add_plural(&mut self, rule: Rule) {
                self.core.custom_plural.push(rule);
            }

            /// The singularisation counterpart of [`Self::add_plural`].
            pub fn add_singular(&mut self, rule: Rule) {
                self.core.custom_singular.push(rule);
            }

            /// Registers `singular`/`plural` as an irregular pair, in both
            /// directions.
            ///
            /// Both arguments are lowercased first, because the lookup tables
            /// are keyed by the lowercase form and case is restored afterwards.
            /// Each direction is a plain overwrite, so re-registering a plural
            /// replaces the singular previously recorded for it. An empty
            /// argument registers nothing usable and is ignored at lookup time.
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
                    .field("added_irregulars", &self.core.added_plural.len())
                    .finish()
            }
        }

        impl SingularPluralInflector for $name {
            fn pluralize(&self, token: &str) -> String {
                Self::pluralize(self, token)
            }
            fn singularize(&self, token: &str) -> String {
                Self::singularize(self, token)
            }
            fn pluralize_into(&self, token: &str, out: &mut String) {
                Self::pluralize_into(self, token, out);
            }
            fn singularize_into(&self, token: &str, out: &mut String) {
                Self::singularize_into(self, token, out);
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
    /// English nouns.
    ///
    /// ```
    /// use verbora_inflectors::NounInflector;
    ///
    /// let inflector = NounInflector::new();
    /// assert_eq!(inflector.pluralize("radius"), "radii");
    /// assert_eq!(inflector.singularize("children"), "child");
    /// // Zero plurals are listed, and the token's own case survives.
    /// assert_eq!(inflector.pluralize("DEER"), "DEER");
    /// // `-man` is the noun *man* only in compounds of it.
    /// assert_eq!(inflector.pluralize("workman"), "workmen");
    /// assert_eq!(inflector.pluralize("human"), "humans");
    /// ```
    NounInflector, EN_FORMS, en, INVARIANT, INVARIANT
}

inflector! {
    /// French nouns.
    ///
    /// ```
    /// use verbora_inflectors::NounInflectorFr;
    ///
    /// let inflector = NounInflectorFr::new();
    /// assert_eq!(inflector.pluralize("cheval"), "chevaux");
    /// assert_eq!(inflector.pluralize("œil"), "yeux");
    /// assert_eq!(inflector.singularize("bijoux"), "bijou");
    /// // A singular already ending in -s, -x or -z is invariant.
    /// assert_eq!(inflector.pluralize("rhinocéros"), "rhinocéros");
    /// assert_eq!(inflector.pluralize("quartz"), "quartz");
    /// ```
    NounInflectorFr, FR_FORMS, fr, INVARIANT, INVARIANT
}

inflector! {
    /// Japanese nouns.
    ///
    /// Japanese does not mark number obligatorily. `pluralize` therefore
    /// *suffixes* rather than inflects: it appends the associative plural 〜たち,
    /// which the language uses productively with animate nouns. `singularize`
    /// removes a plural or associative suffix — 〜たち, 〜達, 〜等, 〜共/〜ども,
    /// 〜方/〜がた — unless the word merely ends in those characters.
    ///
    /// ```
    /// use verbora_inflectors::NounInflectorJa;
    ///
    /// let inflector = NounInflectorJa::new();
    /// assert_eq!(inflector.pluralize("私"), "私たち");
    /// assert_eq!(inflector.singularize("人たち"), "人");
    /// assert_eq!(inflector.pluralize("人"), "人々");
    /// // かたち ("shape") only looks like a plural.
    /// assert_eq!(inflector.singularize("かたち"), "かたち");
    /// ```
    NounInflectorJa, JA_FORMS, ja, PLURAL_INVARIANT, SINGULAR_INVARIANT
}

inflector! {
    /// English verbs, inflected for number agreement.
    ///
    /// The direction names follow the subject, not the verb: `singularize`
    /// produces the form that agrees with a singular subject — the third-person
    /// singular present — and `pluralize` produces the plain form.
    ///
    /// Only the present tense marks number in English, with one exception: *be*
    /// marks it in the past as well, which is why `was`/`were` are here.
    ///
    /// ```
    /// use verbora_inflectors::PresentVerbInflector;
    ///
    /// let inflector = PresentVerbInflector::new();
    /// assert_eq!(inflector.singularize("go"), "goes");
    /// assert_eq!(inflector.pluralize("catches"), "catch");
    /// assert_eq!(inflector.singularize("are"), "is");
    /// // Modal auxiliaries have no third-person singular form.
    /// assert_eq!(inflector.singularize("will"), "will");
    /// ```
    PresentVerbInflector, VERB_FORMS, verb, INVARIANT, INVARIANT
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rule::Rule;

    // ------------------------------------------------------------------
    // Table enumeration. Every entry of every table, every time.
    // ------------------------------------------------------------------

    /// The eight rule tables, with the inflector that owns each one.
    fn rule_tables() -> Vec<(&'static str, &'static [RawRule], FormSet)> {
        vec![
            (
                "en plural",
                data::en::PLURAL_REGULAR,
                FormSet::build(
                    data::en::INVARIANT,
                    data::en::PLURAL_IRREGULAR,
                    data::en::PLURAL_REGULAR,
                ),
            ),
            (
                "en singular",
                data::en::SINGULAR_REGULAR,
                FormSet::build(
                    data::en::INVARIANT,
                    data::en::SINGULAR_IRREGULAR,
                    data::en::SINGULAR_REGULAR,
                ),
            ),
            (
                "fr plural",
                data::fr::PLURAL_REGULAR,
                FormSet::build(
                    data::fr::INVARIANT,
                    data::fr::PLURAL_IRREGULAR,
                    data::fr::PLURAL_REGULAR,
                ),
            ),
            (
                "fr singular",
                data::fr::SINGULAR_REGULAR,
                FormSet::build(
                    data::fr::INVARIANT,
                    data::fr::SINGULAR_IRREGULAR,
                    data::fr::SINGULAR_REGULAR,
                ),
            ),
            (
                "ja plural",
                data::ja::PLURAL_REGULAR,
                FormSet::build(
                    data::ja::PLURAL_INVARIANT,
                    data::ja::PLURAL_IRREGULAR,
                    data::ja::PLURAL_REGULAR,
                ),
            ),
            (
                "ja singular",
                data::ja::SINGULAR_REGULAR,
                FormSet::build(
                    data::ja::SINGULAR_INVARIANT,
                    data::ja::SINGULAR_IRREGULAR,
                    data::ja::SINGULAR_REGULAR,
                ),
            ),
            (
                "verb plural",
                data::verb::PLURAL_REGULAR,
                FormSet::build(
                    data::verb::INVARIANT,
                    data::verb::PLURAL_IRREGULAR,
                    data::verb::PLURAL_REGULAR,
                ),
            ),
            (
                "verb singular",
                data::verb::SINGULAR_REGULAR,
                FormSet::build(
                    data::verb::INVARIANT,
                    data::verb::SINGULAR_IRREGULAR,
                    data::verb::SINGULAR_REGULAR,
                ),
            ),
        ]
    }

    /// The four lexical lists, in every direction they are used.
    fn word_lists() -> Vec<(&'static str, &'static [&'static str])> {
        vec![
            ("en INVARIANT", data::en::INVARIANT),
            ("fr INVARIANT", data::fr::INVARIANT),
            ("ja PLURAL_INVARIANT", data::ja::PLURAL_INVARIANT),
            ("ja SINGULAR_INVARIANT", data::ja::SINGULAR_INVARIANT),
            ("verb INVARIANT", data::verb::INVARIANT),
        ]
    }

    fn irregular_tables() -> Vec<(&'static str, &'static [(&'static str, &'static str)])> {
        vec![
            ("en PLURAL_IRREGULAR", data::en::PLURAL_IRREGULAR),
            ("en SINGULAR_IRREGULAR", data::en::SINGULAR_IRREGULAR),
            ("fr PLURAL_IRREGULAR", data::fr::PLURAL_IRREGULAR),
            ("fr SINGULAR_IRREGULAR", data::fr::SINGULAR_IRREGULAR),
            ("ja PLURAL_IRREGULAR", data::ja::PLURAL_IRREGULAR),
            ("ja SINGULAR_IRREGULAR", data::ja::SINGULAR_IRREGULAR),
            ("verb PLURAL_IRREGULAR", data::verb::PLURAL_IRREGULAR),
            ("verb SINGULAR_IRREGULAR", data::verb::SINGULAR_IRREGULAR),
        ]
    }

    /// Every lexical list must be lowercase and in ascending byte order, since
    /// lookup lowercases the token and then binary-searches. An entry that
    /// breaks either property is unreachable — the defect this project has hit
    /// seven times.
    #[test]
    fn every_lexical_entry_is_reachable() {
        for (name, list) in word_lists() {
            assert!(!list.is_empty(), "{name} is empty");
            for w in list {
                assert_eq!(*w, w.to_lowercase(), "{name}: {w:?} is not lowercase");
                assert!(!w.is_empty(), "{name} contains an empty entry");
            }
            for pair in list.windows(2) {
                assert!(
                    pair[0] < pair[1],
                    "{name}: {:?} must sort before {:?}",
                    pair[0],
                    pair[1]
                );
            }
            // Reachability, stated as the operation the engine performs.
            for w in list {
                assert!(
                    list.binary_search(w).is_ok(),
                    "{name}: {w:?} is unreachable by binary search"
                );
            }
        }
    }

    #[test]
    fn every_irregular_entry_is_reachable() {
        for (name, table) in irregular_tables() {
            for (k, v) in table {
                assert_eq!(*k, k.to_lowercase(), "{name}: key {k:?} is not lowercase");
                assert_eq!(*v, v.to_lowercase(), "{name}: value {v:?} is not lowercase");
                assert!(!k.is_empty() && !v.is_empty(), "{name} has an empty side");
                assert_ne!(k, v, "{name}: {k:?} maps to itself");
            }
            for pair in table.windows(2) {
                assert!(
                    pair[0].0 < pair[1].0,
                    "{name}: {:?} must sort before {:?}",
                    pair[0].0,
                    pair[1].0
                );
            }
            for (k, _) in table {
                assert!(
                    table.binary_search_by_key(k, |(k, _)| k).is_ok(),
                    "{name}: {k:?} is unreachable by binary search"
                );
            }
        }
    }

    /// Each `-IRREGULAR` table must be the inverse of its partner: a pair that
    /// exists in only one direction is a silent asymmetry.
    #[test]
    fn irregular_tables_are_mutual_inverses() {
        for (name, plural, singular) in [
            (
                "en",
                data::en::PLURAL_IRREGULAR,
                data::en::SINGULAR_IRREGULAR,
            ),
            (
                "fr",
                data::fr::PLURAL_IRREGULAR,
                data::fr::SINGULAR_IRREGULAR,
            ),
            (
                "ja",
                data::ja::PLURAL_IRREGULAR,
                data::ja::SINGULAR_IRREGULAR,
            ),
        ] {
            for (s, p) in plural {
                let back = singular
                    .iter()
                    .find(|(k, _)| k == p)
                    .unwrap_or_else(|| panic!("{name}: {p:?} has no singular entry"));
                assert_eq!(back.1, *s, "{name}: {p:?} maps back to the wrong singular");
            }
        }
        // The verb tables are deliberately asymmetric: `am` and `is` both
        // pluralise to `are`, which singularises to `is` alone, and `be` is a
        // plain form with no third-person singular counterpart pointing back.
        assert_eq!(data::verb::PLURAL_IRREGULAR.len(), 4);
        assert_eq!(data::verb::SINGULAR_IRREGULAR.len(), 4);
    }

    /// Every rule compiles. This is what makes the `expect` in `compile_raw`
    /// unreachable rather than merely unlikely.
    #[test]
    fn every_rule_compiles() {
        let mut count = 0;
        for (_, raw, _) in rule_tables() {
            for r in raw {
                match r.replacement {
                    Some(replacement) => {
                        Rule::new(r.pattern, replacement)
                            .unwrap_or_else(|e| panic!("`{}`: {e}", r.pattern));
                    }
                    None => {
                        Rule::keep(r.pattern).unwrap_or_else(|e| panic!("`{}`: {e}", r.pattern));
                    }
                }
                count += 1;
            }
        }
        assert_eq!(count, 81, "rule count changed; update the enumeration");
    }

    /// **The reachability enumeration.** For every rule of every table, the
    /// rule's witness must reach exactly that rule: no earlier rule may claim
    /// it, the lexical lists and the irregular table must not claim it either,
    /// and the whole table must turn it into the recorded form.
    ///
    /// A rule whose witness fails any of these is dead — that is how the
    /// unreachable `ives$` and `$zz` entries this crate used to carry were
    /// found — and a dead rule is deleted, never documented.
    #[test]
    fn every_rule_is_reachable_through_its_witness() {
        for (name, raw, forms) in rule_tables() {
            for (index, entry) in raw.iter().enumerate() {
                let witness = entry.witness;
                assert_eq!(
                    witness,
                    witness.to_lowercase(),
                    "{name}[{index}]: witness {witness:?} must be lowercase so that case \
                     restoration is the identity"
                );
                assert!(
                    !forms.is_invariant(witness),
                    "{name}[{index}]: witness {witness:?} never reaches the rules — the \
                     invariant list claims it first"
                );
                assert!(
                    forms.irregular(witness).is_none(),
                    "{name}[{index}]: witness {witness:?} never reaches the rules — the \
                     irregular table claims it first"
                );
                for (earlier, rule) in forms.regular.iter().enumerate().take(index) {
                    assert!(
                        rule.apply_inner(witness).is_none(),
                        "{name}[{index}] is unreachable: witness {witness:?} is claimed by \
                         {name}[{earlier}] (`{}`)",
                        rule.pattern()
                    );
                }
                let rule = &forms.regular[index];
                assert!(
                    rule.apply_inner(witness).is_some(),
                    "{name}[{index}] (`{}`) does not match its own witness {witness:?}",
                    rule.pattern()
                );
                let mut out = String::new();
                inflect_into(witness, &forms, &[], &[], &mut out);
                assert_eq!(
                    out,
                    entry.expected,
                    "{name}[{index}] (`{}`): {witness:?} must inflect to {:?}",
                    rule.pattern(),
                    entry.expected
                );
            }
        }
    }

    /// A lexical list entry that the rules would leave alone anyway is dead
    /// weight, not a defect — but the count must be visible rather than
    /// discovered later. Zero is asserted for the lists that claim to be
    /// load-bearing.
    #[test]
    fn every_invariant_entry_is_load_bearing() {
        let en = NounInflector::new();
        for word in data::en::INVARIANT {
            // Removing the word from the list must change at least one
            // direction, otherwise the entry does nothing.
            let plural_without = rules_only(&EN_FORMS.plural, word);
            let singular_without = rules_only(&EN_FORMS.singular, word);
            assert!(
                plural_without != *word || singular_without != *word,
                "en INVARIANT: {word:?} is redundant — the rules already leave it alone"
            );
            assert_eq!(en.pluralize(word), *word);
            assert_eq!(en.singularize(word), *word);
        }

        let fr = NounInflectorFr::new();
        let mut fr_redundant_in_plural = 0;
        for word in data::fr::INVARIANT {
            assert!(
                word.ends_with('s') || word.ends_with('x') || word.ends_with('z'),
                "fr INVARIANT: {word:?} does not end in -s, -x or -z"
            );
            if rules_only(&FR_FORMS.plural, word) == *word {
                fr_redundant_in_plural += 1;
            }
            // The singular direction is where the list earns its place, so
            // measure it rather than assume it: an entry the singular rules
            // would already leave alone does nothing at all, since the plural
            // direction is redundant for every entry. 149 such entries (all 37
            // in `-z` and 112 of the 139 in `-x`) were carried until the rules
            // were walked; the guard rule now states their invariance instead.
            assert_ne!(
                rules_only(&FR_FORMS.singular, word),
                **word,
                "fr INVARIANT: {word:?} is redundant — the singular rules already \
                 leave it alone, and the plural direction needs no entry at all"
            );
            assert_eq!(fr.pluralize(word), *word);
            assert_eq!(fr.singularize(word), *word);
        }
        // Every French entry is redundant in the plural direction, because the
        // `-s`/`-x`/`-z` guard rule already covers it. The list exists for the
        // singular direction alone — which the `assert_ne!` above now proves
        // entry by entry, so "redundant in one direction" can never quietly
        // become "redundant in both".
        assert_eq!(fr_redundant_in_plural, data::fr::INVARIANT.len());

        let ja = NounInflectorJa::new();
        for word in data::ja::SINGULAR_INVARIANT {
            assert_ne!(
                rules_only(&JA_FORMS.singular, word),
                *word,
                "ja SINGULAR_INVARIANT: {word:?} is redundant"
            );
            assert_eq!(ja.singularize(word), *word);
        }

        let verb = PresentVerbInflector::new();
        for word in data::verb::INVARIANT {
            assert_eq!(verb.singularize(word), *word);
            assert_eq!(verb.pluralize(word), *word);
        }
    }

    /// A sample of the 149 entries `fr::INVARIANT` used to carry and no longer
    /// does: French nouns in a plain `-x` or in `-z`, which no rule in either
    /// direction rewrites, so enumerating them changed nothing. They must
    /// still be invariant in both directions — now because `PLURAL_REGULAR`'s
    /// `(s|x|z)$` guard and `SINGULAR_REGULAR`'s `(x|z)$` guard say so, not
    /// because a lexicon lists them.
    ///
    /// The sample spans every shape those entries had: bare `-x`, `-yx`,
    /// `-ax`, `-ex`, an `-oux` that the `-ou → -oux` alternation does not
    /// claim, hyphenated compounds, and `-z`. Mixed case is included because
    /// the lexical lists are keyed on the lowercase form while the rules are
    /// not, so the two paths could disagree.
    #[test]
    fn french_nouns_in_plain_x_or_z_are_invariant_by_rule_not_by_lexicon() {
        let fr = NounInflectorFr::new();
        for word in [
            "afflux",
            "anthrax",
            "apex",
            "aptéryx",
            "silex",
            "index",
            "lynx",
            "prix",
            "choix",
            "époux",
            "abat-voix",
            "allume-gaz",
            "quartz",
            "assez",
            "gaz",
            "nez",
            "riz",
            "jazz",
        ] {
            assert!(
                !FR_FORMS.plural.is_invariant(word) && !FR_FORMS.singular.is_invariant(word),
                "{word:?} is back on the lexical list; the point is that it needs no entry"
            );
            assert_eq!(fr.pluralize(word), word, "plural of {word:?}");
            assert_eq!(fr.singularize(word), word, "singular of {word:?}");
            assert_eq!(rules_only(&FR_FORMS.plural, word), word);
            assert_eq!(rules_only(&FR_FORMS.singular, word), word);
        }
        // Case restoration must not smuggle a change in either.
        assert_eq!(fr.pluralize("AFFLUX"), "AFFLUX");
        assert_eq!(fr.singularize("Anthrax"), "Anthrax");

        // The `-x` forms a singular rule *does* claim still need their entries,
        // and the guard must not shadow those rules by sitting before them.
        for (plural, singular) in [("chevaux", "cheval"), ("cadeaux", "cadeau")] {
            assert_eq!(fr.singularize(plural), singular);
        }
        for invariant in ["faux", "vieux", "taux", "chaux"] {
            assert!(
                FR_FORMS.singular.is_invariant(invariant),
                "{invariant:?} must stay on the list: `aux$`/`(eau|eu|œu)x$` would rewrite it"
            );
            assert_eq!(fr.singularize(invariant), invariant);
        }
    }

    /// What the ordered rules alone would produce, bypassing the lexical lists.
    fn rules_only(forms: &FormSet, token: &str) -> String {
        for rule in &forms.regular {
            match rule.apply_inner(token) {
                Some(Applied::Unchanged) => return token.to_owned(),
                Some(Applied::Form(f)) => return f,
                None => {}
            }
        }
        token.to_owned()
    }

    // ------------------------------------------------------------------
    // Engine contract
    // ------------------------------------------------------------------

    #[test]
    fn the_empty_token_is_returned_unchanged_by_every_inflector() {
        assert_eq!(NounInflector::new().pluralize(""), "");
        assert_eq!(NounInflector::new().singularize(""), "");
        assert_eq!(NounInflectorFr::new().pluralize(""), "");
        assert_eq!(NounInflectorJa::new().pluralize(""), "");
        assert_eq!(PresentVerbInflector::new().singularize(""), "");
    }

    #[test]
    fn a_rule_that_would_empty_the_token_is_skipped() {
        let verbs = PresentVerbInflector::new();
        // `(?i)e?s$ -> ""` matches but would erase the token.
        assert_eq!(verbs.pluralize("s"), "s");
        assert_eq!(verbs.pluralize("es"), "es");
        assert_eq!(verbs.pluralize("Es"), "Es");
        assert_eq!(NounInflectorFr::new().singularize("s"), "s");

        let mut nouns = NounInflector::new();
        nouns.add_plural(Rule::new("(?i)^cat$", "").unwrap());
        assert_eq!(nouns.pluralize("cat"), "cats");
    }

    #[test]
    fn custom_rules_outrank_every_built_in_table() {
        let mut nouns = NounInflector::new();
        nouns.add_plural(Rule::new("(?i)deer", "deerz").unwrap());
        nouns.add_singular(Rule::new("(?i)child", "childx").unwrap());
        assert_eq!(nouns.pluralize("deer"), "deerz");
        assert_eq!(nouns.singularize("children"), "childxren");

        // Strictly per-instance.
        let fresh = NounInflector::new();
        assert_eq!(fresh.pluralize("deer"), "deer");
        assert_eq!(fresh.singularize("children"), "child");
    }

    #[test]
    fn earlier_custom_rules_win() {
        let mut nouns = NounInflector::new();
        nouns.add_plural(Rule::new("(?i)o", "FIRST").unwrap());
        nouns.add_plural(Rule::new("(?i)o", "SECOND").unwrap());
        // Preserve mode: the rule's own capitals survive.
        assert_eq!(nouns.pluralize("dog"), "dFIRSTg");
    }

    #[test]
    fn runtime_irregulars_shadow_the_static_table_and_lowercase_both_arguments() {
        let mut nouns = NounInflector::new();
        nouns.add_irregular("MOUSE", "MOUSES");
        assert_eq!(nouns.pluralize("mouse"), "mouses");
        assert_eq!(nouns.singularize("mouses"), "mouse");
        // The built-in reverse mapping is untouched.
        assert_eq!(nouns.singularize("mice"), "mouse");

        // A second write to the same plural key replaces the singular.
        nouns.add_irregular("rat", "mouses");
        assert_eq!(nouns.singularize("mouses"), "rat");

        // An empty side registers nothing usable and is ignored.
        let mut other = NounInflector::new();
        other.add_irregular("ghost", "");
        assert_eq!(other.pluralize("ghost"), "ghosts");
    }

    #[test]
    fn the_buffer_api_appends_and_never_clears() {
        let nouns = NounInflector::new();
        let mut buf = String::from("<");
        nouns.pluralize_into("hacker", &mut buf);
        nouns.singularize_into("children", &mut buf);
        assert_eq!(buf, "<hackerschild");
        // The empty token appends nothing.
        nouns.pluralize_into("", &mut buf);
        assert_eq!(buf, "<hackerschild");
    }

    #[test]
    fn generic_code_can_use_the_trait() {
        fn plural_of<I: SingularPluralInflector>(i: &I, t: &str) -> String {
            i.pluralize(t)
        }
        assert_eq!(plural_of(&NounInflector::new(), "party"), "parties");
        assert_eq!(plural_of(&NounInflectorFr::new(), "cheval"), "chevaux");
    }

    #[test]
    fn debug_reports_the_instance_additions() {
        let mut nouns = NounInflector::new();
        nouns.add_plural(Rule::new("(?i)x$", "xen").unwrap());
        nouns.add_irregular("a", "b");
        let text = format!("{nouns:?}");
        assert!(text.contains("custom_plural_rules: 1"), "{text}");
        assert!(text.contains("added_irregulars: 1"), "{text}");
    }
}
