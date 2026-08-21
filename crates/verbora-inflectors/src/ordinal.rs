use std::fmt;
use std::fmt::Write as _;

/// English ordinal numerals in figures: `1st`, `2nd`, `3rd`, `4th`.
///
/// # The rule
///
/// The suffix is chosen from the **last two decimal digits of the magnitude**:
///
/// | Last two digits | Suffix |
/// |---|---|
/// | `11`, `12`, `13` | `th` |
/// | otherwise, last digit `1` | `st` |
/// | otherwise, last digit `2` | `nd` |
/// | otherwise, last digit `3` | `rd` |
/// | otherwise | `th` |
///
/// This is the form given by *The Chicago Manual of Style* and by *New Hart's
/// Rules*: `21st`, `102nd`, but `111th`, `112th`, `113th`, because the teens are
/// pronounced *eleventh*, *twelfth*, *thirteenth*.
///
/// **The sign is orthographic and does not affect the suffix.** `nth(-1)` is
/// `"-1st"`, read *minus first*, not `"-1th"`. Taking the remainder of a
/// negative number and letting the sign fall through to the default case would
/// give every negative ordinal `th`, which matches no reading of the numeral.
///
/// # Domain
///
/// Ordinals are defined for integers, so the argument is an [`i64`] and nothing
/// else. There is no floating-point entry point: `1.5th` is not an ordinal, and
/// a function that produced one would be inventing a form the language does not
/// have. There is no string entry point either: a string is not a number, and
/// coercing one would make `nth("abc")` answerable.
///
/// Exact across the whole of `i64`, including [`i64::MIN`].
///
/// ```
/// use verbora_inflectors::OrdinalInflector;
///
/// assert_eq!(OrdinalInflector::nth(1), "1st");
/// assert_eq!(OrdinalInflector::nth(22), "22nd");
/// assert_eq!(OrdinalInflector::nth(112), "112th");
/// assert_eq!(OrdinalInflector::nth(-1), "-1st");
/// ```
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct OrdinalInflector;

impl OrdinalInflector {
    /// The two-letter suffix for `n`, without the numeral.
    ///
    /// Use this when the numeral is formatted elsewhere — inside a table cell,
    /// or with thousands separators — and only the suffix is needed. It borrows
    /// a `&'static str` and allocates nothing.
    #[must_use]
    pub const fn suffix(n: i64) -> &'static str {
        match n.unsigned_abs() % 100 {
            11..=13 => "th",
            last_two => match last_two % 10 {
                1 => "st",
                2 => "nd",
                3 => "rd",
                _ => "th",
            },
        }
    }

    /// The full ordinal: the decimal numeral followed by its suffix.
    ///
    /// See [Choosing the right API](crate#choosing-the-right-api) for when to
    /// prefer [`Self::nth_into`].
    #[must_use]
    pub fn nth(n: i64) -> String {
        // 20 digits plus a sign is the widest i64, and the suffix is two bytes:
        // one allocation, never resized.
        let mut out = String::with_capacity(24);
        Self::nth_into(n, &mut out);
        out
    }

    /// [`Self::nth`], **appending** to a caller-owned buffer.
    ///
    /// `out` is never cleared.
    pub fn nth_into(n: i64, out: &mut String) {
        // Writing into the buffer rather than through `n.to_string()` avoids a
        // second allocation. `write!` to a `String` is infallible.
        let _ = write!(out, "{n}");
        out.push_str(Self::suffix(n));
    }
}

/// Grammatical gender, for the languages whose ordinals agree with their noun.
///
/// Only French needs it in this crate: *premier* is masculine and *première*
/// feminine, abbreviated `1er` and `1re`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum Gender {
    /// Masculine agreement.
    #[default]
    Masculine,
    /// Feminine agreement.
    Feminine,
}

impl fmt::Display for Gender {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Masculine => "masculine",
            Self::Feminine => "feminine",
        })
    }
}

/// French ordinal numerals in figures: `1er`, `1re`, `2e`, `3e`.
///
/// # The rule
///
/// Only *premier*/*première* has a suffix of its own. Every other ordinal is
/// formed with *-ième* and abbreviated `e`, whatever the gender and whatever the
/// number: `2e`, `3e`, `21e`, `100e`. This is the abbreviation prescribed by
/// *Le Bon Usage* and by the *Dictionnaire de l'Académie française*.
///
/// Note that `21` is `21e`, not `21er`: *vingt et unième* is an `-ième` form.
/// The suffix depends on the whole number being exactly one in magnitude, not on
/// its last digit.
///
/// As in English, the sign is orthographic: `nth(-1, …)` is `"-1er"`.
///
/// ```
/// use verbora_inflectors::{Gender, OrdinalInflectorFr};
///
/// assert_eq!(OrdinalInflectorFr::nth(1, Gender::Masculine), "1er");
/// assert_eq!(OrdinalInflectorFr::nth(1, Gender::Feminine), "1re");
/// assert_eq!(OrdinalInflectorFr::nth(2, Gender::Feminine), "2e");
/// assert_eq!(OrdinalInflectorFr::nth(21, Gender::Masculine), "21e");
/// ```
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct OrdinalInflectorFr;

impl OrdinalInflectorFr {
    /// The suffix for `n` under `gender`, without the numeral.
    #[must_use]
    pub const fn suffix(n: i64, gender: Gender) -> &'static str {
        if n.unsigned_abs() == 1 {
            match gender {
                Gender::Masculine => "er",
                Gender::Feminine => "re",
            }
        } else {
            "e"
        }
    }

    /// The full ordinal: the decimal numeral followed by its suffix.
    #[must_use]
    pub fn nth(n: i64, gender: Gender) -> String {
        let mut out = String::with_capacity(24);
        Self::nth_into(n, gender, &mut out);
        out
    }

    /// [`Self::nth`], **appending** to a caller-owned buffer.
    ///
    /// `out` is never cleared.
    pub fn nth_into(n: i64, gender: Gender, out: &mut String) {
        let _ = write!(out, "{n}");
        out.push_str(Self::suffix(n, gender));
    }
}

#[cfg(test)]
mod tests {
    use super::{Gender, OrdinalInflector, OrdinalInflectorFr};

    #[test]
    fn english_units_and_the_teens_exception() {
        assert_eq!(OrdinalInflector::nth(0), "0th");
        assert_eq!(OrdinalInflector::nth(1), "1st");
        assert_eq!(OrdinalInflector::nth(2), "2nd");
        assert_eq!(OrdinalInflector::nth(3), "3rd");
        assert_eq!(OrdinalInflector::nth(4), "4th");
        assert_eq!(OrdinalInflector::nth(11), "11th");
        assert_eq!(OrdinalInflector::nth(12), "12th");
        assert_eq!(OrdinalInflector::nth(13), "13th");
        assert_eq!(OrdinalInflector::nth(14), "14th");
        assert_eq!(OrdinalInflector::nth(21), "21st");
        assert_eq!(OrdinalInflector::nth(22), "22nd");
        assert_eq!(OrdinalInflector::nth(23), "23rd");
        assert_eq!(OrdinalInflector::nth(101), "101st");
        assert_eq!(OrdinalInflector::nth(111), "111th");
        assert_eq!(OrdinalInflector::nth(112), "112th");
        assert_eq!(OrdinalInflector::nth(113), "113th");
        assert_eq!(OrdinalInflector::nth(121), "121st");
        assert_eq!(OrdinalInflector::nth(1013), "1013th");
    }

    /// The suffix depends on the last two digits, so the whole rule is settled
    /// by enumerating `0..100` and checking that every higher number agrees
    /// with its own last two digits.
    #[test]
    fn the_suffix_depends_only_on_the_last_two_digits() {
        for n in 0..100i64 {
            let expected = match n {
                11..=13 => "th",
                _ => match n % 10 {
                    1 => "st",
                    2 => "nd",
                    3 => "rd",
                    _ => "th",
                },
            };
            assert_eq!(OrdinalInflector::suffix(n), expected, "{n}");
            for hundreds in [100i64, 1_000, 12_300, 9_007_199_254_740_900] {
                assert_eq!(
                    OrdinalInflector::suffix(hundreds + n),
                    expected,
                    "{}",
                    hundreds + n
                );
            }
        }
    }

    #[test]
    fn the_sign_is_orthographic() {
        // The magnitude decides, so a negative ordinal reads the way it is said.
        assert_eq!(OrdinalInflector::nth(-1), "-1st");
        assert_eq!(OrdinalInflector::nth(-2), "-2nd");
        assert_eq!(OrdinalInflector::nth(-3), "-3rd");
        assert_eq!(OrdinalInflector::nth(-4), "-4th");
        assert_eq!(OrdinalInflector::nth(-11), "-11th");
        assert_eq!(OrdinalInflector::nth(-21), "-21st");
        for n in 1..500i64 {
            assert_eq!(
                OrdinalInflector::suffix(-n),
                OrdinalInflector::suffix(n),
                "{n}"
            );
        }
    }

    /// `i64::MIN` has no positive counterpart, so a magnitude computed with
    /// negation would overflow. `unsigned_abs` is why this is total.
    #[test]
    fn the_extremes_of_i64_are_exact() {
        assert_eq!(OrdinalInflector::nth(i64::MAX), "9223372036854775807th");
        assert_eq!(OrdinalInflector::nth(i64::MIN), "-9223372036854775808th");
        // 9_223_372_036_854_775_808 mod 100 == 8, so the suffix is `th`.
        assert_eq!(OrdinalInflector::suffix(i64::MIN), "th");
        // A value beyond f64's exact integer range, which is why the argument is
        // an integer type.
        assert_eq!(
            OrdinalInflector::nth(9_007_199_254_740_993),
            "9007199254740993rd"
        );
    }

    #[test]
    fn nth_into_appends() {
        let mut buf = String::from("the ");
        OrdinalInflector::nth_into(3, &mut buf);
        buf.push_str(" of ");
        OrdinalInflectorFr::nth_into(1, Gender::Feminine, &mut buf);
        assert_eq!(buf, "the 3rd of 1re");
    }

    #[test]
    fn french_marks_only_the_first() {
        assert_eq!(OrdinalInflectorFr::nth(1, Gender::Masculine), "1er");
        assert_eq!(OrdinalInflectorFr::nth(1, Gender::Feminine), "1re");
        assert_eq!(OrdinalInflectorFr::nth(-1, Gender::Masculine), "-1er");
        for n in [0i64, 2, 3, 11, 21, 100, 1_000_000] {
            assert_eq!(
                OrdinalInflectorFr::nth(n, Gender::Masculine),
                format!("{n}e")
            );
            assert_eq!(
                OrdinalInflectorFr::nth(n, Gender::Feminine),
                format!("{n}e")
            );
        }
        assert_eq!(OrdinalInflectorFr::nth(i64::MIN, Gender::Feminine), {
            let mut s = i64::MIN.to_string();
            s.push('e');
            s
        });
    }

    #[test]
    fn gender_defaults_to_masculine() {
        assert_eq!(Gender::default(), Gender::Masculine);
        assert_eq!(Gender::Feminine.to_string(), "feminine");
    }
}
