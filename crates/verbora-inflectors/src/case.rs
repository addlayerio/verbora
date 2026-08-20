use std::fmt;

/// How the case of an input token is carried onto its inflected form.
///
/// An inflector rewrites a *lowercase* form — `child` becomes `children`, `-ies`
/// replaces `-y` — but the caller's token may be capitalised or upper-cased.
/// [`CaseMode::of`] classifies the token, and [`CaseMode::apply`] puts that
/// classification back onto whatever the rules produced.
///
/// # The text unit
///
/// **One Unicode scalar value is one character.** Classification iterates
/// [`char`]s, never UTF-16 code units and never bytes, so an astral scalar such
/// as `U+1F44D 👍` is exactly one character and the classification of `"👍"` is
/// the classification of a single caseless character.
///
/// A character is **cased** when [`char::is_uppercase`] or [`char::is_lowercase`]
/// holds — the Unicode `Uppercase` and `Lowercase` derived properties (UAX #44).
/// Digits, punctuation, symbols, emoji and every CJK ideograph are uncased, and
/// uncased characters are ignored entirely when choosing a mode.
///
/// # The classification
///
/// Applied to the token in order, first match winning:
///
/// | Condition on the token's cased characters | Mode |
/// |---|---|
/// | none at all | [`Preserve`](CaseMode::Preserve) |
/// | two or more, every one uppercase | [`Upper`](CaseMode::Upper) |
/// | the first uppercase, every later one lowercase | [`Title`](CaseMode::Title) |
/// | anything else | [`Preserve`](CaseMode::Preserve) |
///
/// The "two or more" clause is why `"A"` is [`Title`](CaseMode::Title) rather
/// than [`Upper`](CaseMode::Upper): one uppercase letter is indistinguishable
/// from a capitalised word, and treating it as a shout would pluralise `"A"` to
/// `"AS"`.
///
/// The final row is the reason the crate never silently re-cases its input:
/// `"iPhone"`, `"McDonald"` and `"aBC"` are all [`Preserve`](CaseMode::Preserve),
/// so their interior capitals survive.
///
/// ```
/// use verbora_inflectors::CaseMode;
///
/// assert_eq!(CaseMode::of("word"), CaseMode::Preserve);
/// assert_eq!(CaseMode::of("Word"), CaseMode::Title);
/// assert_eq!(CaseMode::of("WORD"), CaseMode::Upper);
/// assert_eq!(CaseMode::of("A"), CaseMode::Title);
/// assert_eq!(CaseMode::of("iPhone"), CaseMode::Preserve);
/// assert_eq!(CaseMode::of("👍"), CaseMode::Preserve);
/// assert_eq!(CaseMode::of(""), CaseMode::Preserve);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Hash)]
pub enum CaseMode {
    /// Emit the produced form exactly as the rules produced it.
    ///
    /// Chosen for tokens with no cased character, for tokens whose first cased
    /// character is lowercase, and for mixed shapes such as `"McDonald"` that
    /// neither [`Title`](CaseMode::Title) nor [`Upper`](CaseMode::Upper)
    /// describes.
    #[default]
    Preserve,
    /// Uppercase the first character of the produced form and leave the rest
    /// alone.
    ///
    /// The mapping is the full Unicode uppercase mapping, so it can lengthen the
    /// string: `"ß"` becomes `"SS"`.
    Title,
    /// Uppercase the whole produced form, using the full Unicode uppercase
    /// mapping.
    Upper,
}

impl CaseMode {
    /// Classifies `token`.
    ///
    /// Total: every string, including `""`, has a mode. See the type
    /// documentation for the classification table.
    #[must_use]
    pub fn of(token: &str) -> Self {
        let mut first_cased_is_upper = None;
        let mut cased = 0usize;
        let mut later_upper = false;
        let mut any_lower = false;

        for c in token.chars() {
            let upper = c.is_uppercase();
            let lower = c.is_lowercase();
            if !upper && !lower {
                continue;
            }
            cased += 1;
            if first_cased_is_upper.is_none() {
                first_cased_is_upper = Some(upper);
            } else if upper {
                later_upper = true;
            }
            any_lower |= lower;
        }

        match first_cased_is_upper {
            None => Self::Preserve,
            Some(false) => Self::Preserve,
            Some(true) if cased >= 2 && !any_lower => Self::Upper,
            Some(true) if !later_upper => Self::Title,
            Some(true) => Self::Preserve,
        }
    }

    /// Applies the mode to `form`, returning a new [`String`].
    ///
    /// # Choosing between `apply` and `apply_into`
    ///
    /// [`apply`](CaseMode::apply) allocates one `String` per call and is the
    /// right choice for one-off use. [`apply_into`](CaseMode::apply_into)
    /// appends to a buffer the caller owns, which lets a loop over a corpus
    /// allocate once instead of once per word; the cost is a buffer to manage
    /// and a `clear()` that is a silent correctness bug when forgotten. Neither
    /// is faster per character — they differ only in allocation.
    #[must_use]
    pub fn apply(self, form: &str) -> String {
        let mut out = String::with_capacity(form.len() + 2);
        self.apply_into(form, &mut out);
        out
    }

    /// Applies the mode to `form`, **appending** to `out`.
    ///
    /// `out` is never cleared. See [`apply`](CaseMode::apply) for when to prefer
    /// which.
    pub fn apply_into(self, form: &str, out: &mut String) {
        match self {
            Self::Preserve => out.push_str(form),
            Self::Upper => push_uppercase(form, out),
            Self::Title => push_titlecase(form, out),
        }
    }
}

impl fmt::Display for CaseMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Preserve => "preserve",
            Self::Title => "title",
            Self::Upper => "upper",
        })
    }
}

/// Full Unicode uppercase mapping, with an allocation-free ASCII path.
fn push_uppercase(s: &str, out: &mut String) {
    if s.is_ascii() {
        out.extend(s.chars().map(|c| c.to_ascii_uppercase()));
    } else {
        out.extend(s.chars().flat_map(char::to_uppercase));
    }
}

/// Uppercases the first character and copies the rest verbatim.
fn push_titlecase(s: &str, out: &mut String) {
    let mut chars = s.chars();
    let Some(first) = chars.next() else {
        return;
    };
    if first.is_ascii() {
        out.push(first.to_ascii_uppercase());
    } else {
        out.extend(first.to_uppercase());
    }
    out.push_str(chars.as_str());
}

#[cfg(test)]
mod tests {
    use super::CaseMode;

    #[test]
    fn empty_token_preserves() {
        assert_eq!(CaseMode::of(""), CaseMode::Preserve);
        assert_eq!(CaseMode::Preserve.apply(""), "");
        assert_eq!(CaseMode::Title.apply(""), "");
        assert_eq!(CaseMode::Upper.apply(""), "");
    }

    #[test]
    fn ascii_classification_table() {
        assert_eq!(CaseMode::of("abc"), CaseMode::Preserve);
        assert_eq!(CaseMode::of("Abc"), CaseMode::Title);
        assert_eq!(CaseMode::of("ABC"), CaseMode::Upper);
        assert_eq!(CaseMode::of("aBC"), CaseMode::Preserve);
        assert_eq!(CaseMode::of("a"), CaseMode::Preserve);
    }

    #[test]
    fn a_single_uppercase_letter_is_title_not_upper() {
        // Two or more cased characters are required for `Upper`, so `pluralize`
        // of a one-letter abbreviation is "As", not "AS".
        assert_eq!(CaseMode::of("A"), CaseMode::Title);
        assert_eq!(CaseMode::Title.apply("as"), "As");
        // Two uppercase letters do reach `Upper`.
        assert_eq!(CaseMode::of("AB"), CaseMode::Upper);
    }

    #[test]
    fn interior_capitals_are_never_rewritten() {
        for token in ["iPhone", "McDonald", "aBC", "eBay", "LaTeX"] {
            assert_eq!(
                CaseMode::of(token),
                CaseMode::Preserve,
                "{token} must keep its own case"
            );
            assert_eq!(CaseMode::Preserve.apply(token), token);
        }
    }

    #[test]
    fn uncased_characters_never_select_a_mode() {
        for token in [
            "1",
            "12",
            "!",
            "!!",
            "私",
            "私たち",
            "👍",
            "👍👍",
            "-",
            "3.14",
        ] {
            assert_eq!(
                CaseMode::of(token),
                CaseMode::Preserve,
                "{token} has no cased character"
            );
        }
        // A cased character among uncased ones still decides.
        assert_eq!(CaseMode::of("1A"), CaseMode::Title);
        assert_eq!(CaseMode::of("1AB"), CaseMode::Upper);
    }

    #[test]
    fn astral_scalars_count_as_one_character() {
        // U+1F44D is one scalar and is uncased, so it cannot make a token
        // "capitalised" the way a UTF-16 surrogate pair would.
        assert_eq!(CaseMode::of("👍"), CaseMode::Preserve);
        assert_eq!(CaseMode::of("👍a"), CaseMode::Preserve);
        assert_eq!(CaseMode::of("👍A"), CaseMode::Title);
        assert_eq!(CaseMode::of("A👍"), CaseMode::Title);
        assert_eq!(CaseMode::of("👍AB"), CaseMode::Upper);
    }

    #[test]
    fn unicode_case_properties_decide() {
        // U+00DF is Lowercase, so it is cased and starts a lowercase word.
        assert_eq!(CaseMode::of("ß"), CaseMode::Preserve);
        assert_eq!(CaseMode::of("straße"), CaseMode::Preserve);
        assert_eq!(CaseMode::of("Straße"), CaseMode::Title);
        assert_eq!(CaseMode::of("İstanbul"), CaseMode::Title);
        assert_eq!(CaseMode::of("ÜBER"), CaseMode::Upper);
        assert_eq!(CaseMode::of("café"), CaseMode::Preserve);
        assert_eq!(CaseMode::of("Œil"), CaseMode::Title);
        assert_eq!(CaseMode::of("ŒIL"), CaseMode::Upper);
    }

    #[test]
    fn applying_a_mode_uses_the_full_unicode_mapping() {
        assert_eq!(CaseMode::Upper.apply("abc"), "ABC");
        assert_eq!(CaseMode::Title.apply("abc"), "Abc");
        assert_eq!(CaseMode::Preserve.apply("aBc"), "aBc");
        // The uppercase mapping of U+00DF is two characters.
        assert_eq!(CaseMode::Upper.apply("ßs"), "SSS");
        assert_eq!(CaseMode::Title.apply("ßs"), "SSs");
        // Greek final sigma: str::to_uppercase is context sensitive, and the
        // per-character mapping used here maps both sigmas to U+03A3.
        assert_eq!(CaseMode::Upper.apply("ας"), "ΑΣ");
    }

    #[test]
    fn apply_into_appends_and_never_clears() {
        let mut buf = String::from("<");
        CaseMode::Title.apply_into("child", &mut buf);
        CaseMode::Upper.apply_into("!x", &mut buf);
        assert_eq!(buf, "<Child!X");
    }
}
