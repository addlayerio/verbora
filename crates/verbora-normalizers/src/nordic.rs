//! `normalizeNo` and `normalizeSv` — the Norwegian and Swedish diacritic folders.
//!
//! Ports of the reference `normalizer_no` and `normalizer_sv`.
//!
//! Both are deliberately narrower than [`crate::remove_diacritics`]: each
//! preserves the letters its own alphabet actually uses. Norwegian keeps
//! `ä ö ü å ø æ`; Swedish keeps those *and* `â ç ê î ñ ó ô û š`, folding only
//! the four `a`/`e` accents.

use std::borrow::Cow;

/// Norwegian folding pairs, in the order `normalizer_no` applies them.
///
/// 26 replacements over 13 letters. The order is recorded faithfully even though
/// it happens not to matter: no replacement output is a later needle, so the
/// passes are independent.
static NO_FOLD: [(char, char); 26] = [
    ('à', 'a'),
    ('À', 'A'),
    ('á', 'a'),
    ('Á', 'A'),
    ('â', 'a'),
    ('Â', 'A'),
    ('ç', 'c'),
    ('Ç', 'C'),
    ('è', 'e'),
    ('È', 'E'),
    ('é', 'e'),
    ('É', 'E'),
    ('ê', 'e'),
    ('Ê', 'E'),
    ('î', 'i'),
    ('Î', 'I'),
    ('ñ', 'n'),
    ('Ñ', 'N'),
    ('ó', 'o'),
    ('Ó', 'O'),
    ('ô', 'o'),
    ('Ô', 'O'),
    ('û', 'u'),
    ('Û', 'U'),
    ('š', 's'),
    ('Š', 'S'),
];

/// Swedish folding pairs, in the order `normalizer_sv` applies them.
static SV_FOLD: [(char, char); 8] = [
    ('à', 'a'),
    ('À', 'A'),
    ('á', 'a'),
    ('Á', 'A'),
    ('è', 'e'),
    ('È', 'E'),
    ('é', 'e'),
    ('É', 'E'),
];

/// Folds Norwegian diacritics, leaving `ä ö ü å ø æ` alone.
///
/// # The first-occurrence quirk
///
/// The reference calls `text.replace('à', 'a')` with a **string** first argument.
/// The reference's `String#replace` only replaces the first occurrence unless it is
/// given a global regular expression, so each of the 26 passes rewrites exactly
/// one character. Rust's [`str::replace`] replaces *all* occurrences, so a direct
/// translation silently diverges on any word with a repeated accent.
///
/// ```
/// # use verbora_normalizers::normalize_no;
/// assert_eq!(normalize_no("ààà"), "aàà");
/// assert_eq!(normalize_no("ééé àà"), "eéé aà");
/// assert_eq!(normalize_no("äöüÅåØø"), "äöüÅåØø");
/// ```
#[must_use]
pub fn normalize_no(text: &str) -> Cow<'_, str> {
    fold_first_of_each(text, &NO_FOLD)
}

/// Folds Swedish diacritics, leaving `ä ö å` alone.
///
/// Same first-occurrence-only semantics as [`normalize_no`]; see there.
///
/// # Divergence from the reference export
///
/// The reference `normalizeSv` is **not callable**: `normalizers/index` exports the
/// whole module object, `{ removeDiacritics: [Function] }`, so
/// The reference `normalizeSv('x')` throws `TypeError: normalizeSv is not a function`
/// — even though the bundled `index.d.ts` declares it as `(str: string) =>
/// string`. The real callable is the reference's `normalizeSv.removeDiacritics`, which
/// is what the Swedish tokenizer uses internally and what this function mirrors.
/// Rust has no analogue of accidentally exporting a module namespace, so the
/// broken top-level export is not reproduced.
///
/// ```
/// # use verbora_normalizers::normalize_sv;
/// assert_eq!(normalize_sv("ààà ééé À É è È"), "aàà eéé A E e E");
/// // Unlike the Norwegian folder, these are left alone.
/// assert_eq!(normalize_sv("âçêîñóôûš"), "âçêîñóôûš");
/// ```
#[must_use]
pub fn normalize_sv(text: &str) -> Cow<'_, str> {
    fold_first_of_each(text, &SV_FOLD)
}

/// The lowest and highest needle across both tables, used as a cheap gate.
///
/// `à` is U+00E0 and `Š` is U+0160; `À` U+00C0 is the true minimum.
const NEEDLE_RANGE: std::ops::RangeInclusive<char> = '\u{00C0}'..='\u{0161}';

/// Replaces the first occurrence of each needle in `table`, in one pass.
///
/// The reference makes one `String#replace` call per pair, which allocates a
/// fresh string 26 times. Because the needles are distinct and no replacement is
/// itself a needle, "first occurrence of each" is order-independent, so a single
/// left-to-right scan carrying a bitmask of already-spent pairs is exactly
/// equivalent and allocates at most once.
fn fold_first_of_each<'a>(text: &'a str, table: &[(char, char)]) -> Cow<'a, str> {
    debug_assert!(table.len() <= 32, "the spent-pair mask is a u32");

    let mut spent: u32 = 0;
    let mut out: Option<String> = None;
    let mut copied = 0usize;

    for (i, c) in text.char_indices() {
        // Every needle is a Latin-1/Latin-Extended-A letter, so ASCII and every
        // other script skip the table scan entirely.
        if !NEEDLE_RANGE.contains(&c) {
            continue;
        }
        let Some(k) = table.iter().position(|&(needle, _)| needle == c) else {
            continue;
        };
        let bit = 1u32 << k;
        if spent & bit != 0 {
            continue;
        }
        spent |= bit;

        let buf = out.get_or_insert_with(|| String::with_capacity(text.len()));
        buf.push_str(&text[copied..i]);
        buf.push(table[k].1);
        copied = i + c.len_utf8();

        // Once every pair has fired the rest of the string is a verbatim copy.
        if spent.count_ones() as usize == table.len() {
            break;
        }
    }

    match out {
        Some(mut buf) => {
            buf.push_str(&text[copied..]);
            Cow::Owned(buf)
        }
        None => Cow::Borrowed(text),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_and_single_characters() {
        assert_eq!(normalize_no(""), "");
        assert_eq!(normalize_sv(""), "");
        assert_eq!(normalize_no("à"), "a");
        assert_eq!(normalize_sv("à"), "a");
        assert_eq!(normalize_no("a"), "a");
    }

    #[test]
    fn only_the_first_occurrence_of_each_needle_folds() {
        assert_eq!(normalize_no("ààà"), "aàà");
        assert_eq!(normalize_no("àbàcà"), "abàcà");
        assert_eq!(normalize_no("ééé àà"), "eéé aà");
        assert_eq!(normalize_sv("ààà ééé À É è È"), "aàà eéé A E e E");
    }

    #[test]
    fn each_pair_has_its_own_first_occurrence() {
        // Upper- and lowercase are separate needles, so both fold once.
        assert_eq!(normalize_no("àÀàÀ"), "aAàÀ");
    }

    #[test]
    fn all_uppercase() {
        assert_eq!(normalize_no("ÀÁÂÇÈÉÊÎÑÓÔÛŠ"), "AAACEEEINOOUS");
    }

    #[test]
    fn preserved_letters() {
        assert_eq!(normalize_no("äöüÅåØøÆæ"), "äöüÅåØøÆæ");
        assert_eq!(normalize_sv("äöåÄÖÅ"), "äöåÄÖÅ");
        // Swedish deliberately does not fold what Norwegian does.
        assert_eq!(normalize_sv("âÂçÇêÊîÎñÑóÓôÔûÛšŠ"), "âÂçÇêÊîÎñÑóÓôÔûÛšŠ");
    }

    #[test]
    fn borrows_when_nothing_changes() {
        assert!(matches!(normalize_no("blabaersyltetoy"), Cow::Borrowed(_)));
        assert!(matches!(normalize_no("日本語"), Cow::Borrowed(_)));
        assert!(matches!(normalize_sv("Москва"), Cow::Borrowed(_)));
    }

    #[test]
    fn decomposed_forms_are_left_alone() {
        // No NFC/NFD step: only precomposed codepoints are recognised.
        assert_eq!(normalize_no("e\u{0301}"), "e\u{0301}");
    }

    #[test]
    fn other_scripts_digits_and_punctuation() {
        assert_eq!(
            normalize_no("Москва Ελλάδα 日本語 123 !?"),
            "Москва Ελλάδα 日本語 123 !?"
        );
    }

    #[test]
    fn astral_characters_survive_a_fold_in_the_same_string() {
        assert_eq!(normalize_no("😀à😀à"), "😀a😀à");
    }

    #[test]
    fn very_long_input_folds_exactly_once() {
        let input = "à".repeat(10_000);
        let out = normalize_no(&input);
        assert!(out.starts_with('a'));
        assert_eq!(out.matches('a').count(), 1);
        assert_eq!(out.matches('à').count(), 9_999);
    }
}
