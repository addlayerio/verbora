//! `TenseInflector.restoreCase`: deciding, from the *original* token, how to
//! re-case whatever the rules produced.
//!
//! The reference is four lines long and every one of them is a trap:
//!
//! ```text
//! if (token[0] === token[0].toUpperCase()) {
//!   if (token[1] && token[1] === token[1].toLowerCase()) return capitalize
//!   else return uppercaseify
//! } else return lowercaseify
//! ```
//!
//! * `token[0]` and `token[1]` are **UTF-16 code units**, not characters. For
//!   `"👍"` the reference sees two units — a high surrogate that is
//!   uppercase-invariant and a low surrogate that is lowercase-invariant — and
//!   therefore picks `capitalize`. A port that iterates `chars()` sees one
//!   character, finds no `token[1]`, and picks `uppercaseify`: `pluralize("👍")`
//!   becomes `"👍S"` instead of `"👍s"`.
//! * The test is a **round-trip string comparison**, not a character-class
//!   query. Digits, punctuation, CJK and surrogates are neither uppercase nor
//!   lowercase, yet `c === c.toUpperCase()` is true for all of them — which is
//!   why `pluralize("1")` is `"1S"` and `pluralize("12")` is `"12s"`. Reaching
//!   for [`char::is_uppercase`] gets both wrong.
//! * The mapping is the *full* one, so it can change length: `"ß".toUpperCase()`
//!   is `"SS"`, which is not `"ß"`, so `"ß"` takes the lowercase branch and
//!   `pluralize("ß")` is `"ßs"`.
//!
//! `capitalize` would throw on an empty string, but [`crate::Rule`] results are
//! never empty (see the falsy-empty-string fallthrough in
//! [`crate::SingularPluralInflector`]), so that path is unreachable through the
//! public API — exactly as in the reference.

/// Which of the three case-restoration functions the reference selected.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CaseMode {
    /// `lowercaseify`: the token began with a lowercase-ish character.
    Lower,
    /// `capitalize`: uppercase-invariant first unit, lowercase-invariant second.
    Capitalize,
    /// `uppercaseify`: uppercase-invariant first unit, and no lowercase-ish
    /// second unit — including the single-character case, where `token[1]` is
    /// `undefined` and therefore falsy.
    Upper,
}

/// Selects the case-restoration mode for `token`.
///
/// Returns `None` for the empty string, which is where the reference throws
/// `TypeError: Cannot read properties of undefined (reading 'toUpperCase')`.
#[must_use]
pub fn restore_case(token: &str) -> Option<CaseMode> {
    let mut chars = token.chars();
    let first = chars.next()?;

    // A non-BMP character occupies two code units. `token[0]` is then the high
    // surrogate, which no case mapping touches, so the first test passes.
    let first_is_astral = first.len_utf16() == 2;
    if !first_is_astral && !is_uppercase_invariant(first) {
        return Some(CaseMode::Lower);
    }

    let second_is_lowercase_invariant = if first_is_astral {
        // `token[1]` is the low surrogate: present, and case-invariant.
        true
    } else {
        match chars.next() {
            // `token[1]` is `undefined`, which is falsy.
            None => false,
            // A non-BMP second character contributes its high surrogate.
            Some(c) => c.len_utf16() == 2 || is_lowercase_invariant(c),
        }
    };

    Some(if second_is_lowercase_invariant {
        CaseMode::Capitalize
    } else {
        CaseMode::Upper
    })
}

impl CaseMode {
    /// Applies the restoration to `s`.
    #[must_use]
    pub fn apply(self, s: &str) -> String {
        let mut out = String::with_capacity(s.len() + 2);
        self.apply_into(s, &mut out);
        out
    }

    /// Applies the restoration, appending to `out`.
    ///
    /// The buffer form exists so the inflectors can produce their result with a
    /// single allocation even though the reference composes three functions.
    pub fn apply_into(self, s: &str, out: &mut String) {
        match self {
            Self::Lower => push_lowercase(s, out),
            Self::Upper => push_uppercase(s, out),
            Self::Capitalize => push_capitalized(s, out),
        }
    }
}

/// `token[0] === token[0].toUpperCase()` for a single BMP character.
///
/// Written as a mapping round trip because that is what the reference compares;
/// `!c.is_lowercase()` would disagree on characters with multi-character or
/// asymmetric mappings.
fn is_uppercase_invariant(c: char) -> bool {
    if c.is_ascii() {
        return !c.is_ascii_lowercase();
    }
    let mut upper = c.to_uppercase();
    upper.next() == Some(c) && upper.next().is_none()
}

/// `token[1] === token[1].toLowerCase()` for a single BMP character.
fn is_lowercase_invariant(c: char) -> bool {
    if c.is_ascii() {
        return !c.is_ascii_uppercase();
    }
    let mut lower = c.to_lowercase();
    lower.next() == Some(c) && lower.next().is_none()
}

/// `String.prototype.toLowerCase`, with an allocation-free ASCII path.
///
/// The non-ASCII path goes through [`str::to_lowercase`] rather than mapping
/// character by character: both Rust and the reference apply the context-sensitive
/// Greek final-sigma rule, and a per-character map would not.
fn push_lowercase(s: &str, out: &mut String) {
    if s.is_ascii() {
        out.extend(s.chars().map(|c| c.to_ascii_lowercase()));
    } else {
        out.push_str(&s.to_lowercase());
    }
}

/// `String.prototype.toUpperCase`, with an allocation-free ASCII path.
fn push_uppercase(s: &str, out: &mut String) {
    if s.is_ascii() {
        out.extend(s.chars().map(|c| c.to_ascii_uppercase()));
    } else {
        out.push_str(&s.to_uppercase());
    }
}

/// `t[0].toUpperCase() + t.slice(1)`, where the index is a UTF-16 code unit.
///
/// When the string starts with a non-BMP character, `t[0]` is a lone high
/// surrogate whose uppercase mapping is itself, so the concatenation rebuilds
/// the original string — which is representable in Rust even though the
/// intermediate lone surrogate is not.
fn push_capitalized(s: &str, out: &mut String) {
    let mut chars = s.chars();
    let Some(first) = chars.next() else {
        // Unreachable through the public API: an empty rule result is discarded
        // by the falsy-empty-string fallthrough before it reaches here.
        return;
    };
    if first.len_utf16() == 2 {
        out.push_str(s);
        return;
    }
    if first.is_ascii() {
        out.push(first.to_ascii_uppercase());
    } else {
        out.extend(first.to_uppercase());
    }
    out.push_str(chars.as_str());
}

#[cfg(test)]
mod tests {
    use super::{CaseMode, restore_case};

    fn mode(token: &str) -> CaseMode {
        restore_case(token).expect("non-empty")
    }

    #[test]
    fn empty_token_has_no_mode() {
        assert_eq!(restore_case(""), None);
    }

    #[test]
    fn ascii_decision_table() {
        assert_eq!(mode("abc"), CaseMode::Lower);
        assert_eq!(mode("Abc"), CaseMode::Capitalize);
        assert_eq!(mode("ABC"), CaseMode::Upper);
        assert_eq!(mode("aBC"), CaseMode::Lower);
        assert_eq!(mode("A"), CaseMode::Upper);
        assert_eq!(mode("a"), CaseMode::Lower);
    }

    #[test]
    fn caseless_characters_take_the_uppercase_branch() {
        assert_eq!(mode("1"), CaseMode::Upper);
        assert_eq!(mode("12"), CaseMode::Capitalize);
        assert_eq!(mode("私"), CaseMode::Upper);
        assert_eq!(mode("私たち"), CaseMode::Capitalize);
        assert_eq!(mode("!"), CaseMode::Upper);
        assert_eq!(mode("!!"), CaseMode::Capitalize);
    }

    #[test]
    fn multi_character_uppercase_mapping_takes_the_lowercase_branch() {
        // "ß".toUpperCase() === "SS", which is not "ß".
        assert_eq!(mode("ß"), CaseMode::Lower);
        assert_eq!(mode("straße"), CaseMode::Lower);
    }

    #[test]
    fn astral_characters_are_counted_as_two_code_units() {
        // Both units are case-invariant, so the reference picks `capitalize`; a
        // `chars()`-based port would pick `uppercaseify`.
        assert_eq!(mode("👍"), CaseMode::Capitalize);
        assert_eq!(mode("👍a"), CaseMode::Capitalize);
        assert_eq!(mode("a👍"), CaseMode::Lower);
        assert_eq!(mode("A👍"), CaseMode::Capitalize);
    }

    #[test]
    fn accented_and_turkish_forms() {
        assert_eq!(mode("İstanbul"), CaseMode::Capitalize);
        assert_eq!(mode("Ångström"), CaseMode::Capitalize);
        assert_eq!(mode("ÜBER"), CaseMode::Upper);
        assert_eq!(mode("café"), CaseMode::Lower);
        assert_eq!(mode("Œil"), CaseMode::Capitalize);
        assert_eq!(mode("ŒIL"), CaseMode::Upper);
    }

    #[test]
    fn applying_a_mode_reproduces_the_reference_functions() {
        assert_eq!(CaseMode::Lower.apply("AbC"), "abc");
        assert_eq!(CaseMode::Upper.apply("abc"), "ABC");
        assert_eq!(CaseMode::Capitalize.apply("abc"), "Abc");
        assert_eq!(CaseMode::Capitalize.apply("👍as"), "👍as");
        assert_eq!(CaseMode::Upper.apply("ßs"), "SSS");
        // Greek final sigma is context sensitive in both languages.
        assert_eq!(CaseMode::Lower.apply("ΑΣ"), "ας");
        assert_eq!(CaseMode::Upper.apply("ας"), "ΑΣ");
    }
}
