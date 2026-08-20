//! `remove_diacritics` — the Verbora-defined combining-mark fold.

use std::borrow::Cow;

use unicode_normalization::UnicodeNormalization;
use unicode_normalization::char::canonical_combining_class;

/// Removes combining marks, leaving the base letters.
///
/// # The definition
///
/// > `remove_diacritics(s)` is `s` under Canonical Decomposition (NFD,
/// > [UAX #15] §1.2), with every scalar whose `Canonical_Combining_Class` is
/// > non-zero removed, under Canonical Composition (NFC).
///
/// `Canonical_Combining_Class` is a property of the Unicode Character Database
/// (The Unicode Standard §4.3, *Combining Classes*), so the result depends on
/// the Unicode version — see [`unicode_version`](crate::unicode_version).
///
/// Three parts of that sentence are load-bearing:
///
/// * **NFD first**, so the answer does not depend on how the caller typed the
///   text. `remove_diacritics("é")` and `remove_diacritics("e\u{0301}")` are
///   both `"e"`.
/// * **`ccc != 0`, not `General_Category ∈ {Mn, Mc, Me}`.** The non-zero
///   classes are exactly the marks canonical ordering reorders, which is the
///   technical sense of "accent". Stripping all marks instead would destroy
///   Thai, Indic and Hangul text wholesale; see "What survives" below for
///   where the two sets differ and what that costs.
/// * **NFC last**, because NFD decomposes Hangul syllables into `ccc = 0`
///   jamo and only composition puts them back. Without it
///   `remove_diacritics("한국")` would return decomposed jamo — a different
///   string that renders identically, which is the class of surprise this
///   function exists to remove rather than create.
///
/// # Guarantees
///
/// * **The output is in NFC.**
/// * **Idempotent.** `remove_diacritics(remove_diacritics(s))` equals
///   `remove_diacritics(s)`, for every `s`.
/// * **Independent of the input's normalization form.**
///   `remove_diacritics(nfd(s))`, `remove_diacritics(s)` and
///   `remove_diacritics(nfc(s))` are all equal.
/// * **Every occurrence folds, not the first.** `remove_diacritics("ààà")` is
///   `"aaa"`.
/// * **Position-independent**, in this exact sense: split `s` into `a + b` at
///   any `char` boundary where the first scalar of `nfd(b)` has
///   `Canonical_Combining_Class` 0. Then
///   `remove_diacritics(s) == nfc(remove_diacritics(a) + remove_diacritics(b))`.
///   The `nfc` is not decoration: canonical composition can act across the
///   join even between two `ccc = 0` scalars — Hangul `L` + `V`, and a handful
///   of Indic and Myanmar pairs — so plain concatenation is *not* equal there.
///   Everywhere those pairs do not occur, which includes all Latin, Greek,
///   Cyrillic, Hebrew and Arabic text, the `nfc` is the identity and the
///   property reads as plain concatenation: the same word folds the same way
///   wherever it appears in a document.
/// * [`Cow::Borrowed`] if and only if the result is byte-identical to `s`.
///
/// # What survives, and why
///
/// The rustdoc leads with the edges rather than burying them, because most
/// surprises with a function like this are at the edges.
///
/// | Input | Result | Reason |
/// |---|---|---|
/// | `ø`, `Æ`, `đ`, `ł`, `ħ`, `ŋ`, `ı` | unchanged | empty `Decomposition_Mapping`; the stroke or bar is part of the letter's identity, not a mark applied to it |
/// | `ß` | `ß` | not a diacritic. `ß → ss` is *case folding*, [UAX #21] |
/// | `Ａ` (fullwidth), `Ⓐ` (circled), `ǅ`, `ſ` | unchanged | *compatibility* decompositions, not canonical. Compose with [`nfkc`](crate::nfkc) first if that is what you want |
/// | `Å` U+212B | `A` | canonical singleton to U+00C5, which decomposes to `A` + U+030A |
/// | `İ` U+0130 | `I` | canonical decomposition `0049 0307`, and `ccc(U+0307) = 230` |
/// | Devanagari matras, Thai `SARA I`-class vowel signs, Hangul jamo | unchanged | `ccc = 0` |
/// | Hebrew niqqud (`ccc` 10–26), Arabic harakat (`ccc` 27–34), Devanagari nukta (`ccc` 7) | removed | non-zero, and this is the operation those scripts call diacritic removal |
/// | Devanagari virama U+094D (`ccc` 9), Thai tone marks (`ccc` 107), Thai `SARA U`/`SARA UU` (`ccc` 103) | **removed** | also non-zero. `ccc != 0` is not the same as "is an accent", and for these scripts the fold changes the word rather than de-accenting it |
///
/// That last row is the honest limit of the definition: `remove_diacritics` is
/// safe to apply blindly to Latin-script text and is *not* safe to apply
/// blindly to Thai or Devanagari. It is a fold for matching and indexing, not
/// a transliteration.
///
/// # Allocation
///
/// Nothing for pure-ASCII input, which returns immediately — ASCII is
/// invariant under all four normalization forms and contains no combining
/// mark, so this is exact rather than a heuristic. Nothing for text that
/// contains no non-zero-class scalar and is already in NFC. One `String`
/// otherwise.
///
/// # Examples
///
/// ```
/// use std::borrow::Cow;
/// use verbora_normalizers::remove_diacritics;
///
/// assert_eq!(remove_diacritics("piñon ça va über résumé"), "pinon ca va uber resume");
/// // Independent of how the text was typed.
/// assert_eq!(remove_diacritics("e\u{0301}"), "e");
/// // Every occurrence, not the first.
/// assert_eq!(remove_diacritics("ààà"), "aaa");
/// // Letters whose mark is part of their identity are left alone.
/// assert_eq!(remove_diacritics("blåbærsyltetøy"), "blabærsyltetøy");
/// // Hangul comes back composed, not as jamo.
/// assert_eq!(remove_diacritics("한국"), "한국");
/// assert!(matches!(remove_diacritics("plain ascii"), Cow::Borrowed(_)));
/// ```
///
/// [UAX #15]: https://www.unicode.org/reports/tr15/#Norm_Forms
/// [UAX #21]: https://www.unicode.org/reports/tr21/
#[must_use]
pub fn remove_diacritics(s: &str) -> Cow<'_, str> {
    // Exact, not a heuristic: every ASCII scalar is its own NFD and NFC and
    // has `Canonical_Combining_Class` 0, so the definition is the identity on
    // ASCII input.
    if s.is_ascii() {
        return Cow::Borrowed(s);
    }

    if !s.nfd().any(|c| canonical_combining_class(c) != 0) {
        // Nothing to remove, so the definition collapses to NFC(NFD(s)), which
        // is NFC(s). Delegating keeps the `Cow` guarantee in one place.
        return crate::nfc(s);
    }

    // The filtered sequence is a subsequence of an NFD string with every
    // non-starter dropped, so it is itself in NFD and differs from NFD(s) —
    // which means the result differs from `s` and `Cow::Owned` is right.
    Cow::Owned(
        s.nfd()
            .filter(|&c| canonical_combining_class(c) == 0)
            .nfc()
            .collect(),
    )
}

/// [`remove_diacritics`], fanned out across a `rayon` thread pool. Requires
/// the `parallel` feature.
///
/// # Why this exists
///
/// `remove_diacritics` is a pure function of one `&str` with no shared state,
/// so folding many independent documents is embarrassingly parallel with no
/// coordination cost between them. The body is exactly
/// `inputs.par_iter().map(remove_diacritics).collect()` — a fan-out over the
/// sequential primitive, not a second implementation of it. If you need a
/// different shape (a shared output buffer, say), apply the same
/// `par_iter().map(…)` pattern at your own call site.
///
/// # When to reach for it
///
/// The crossover — the batch size and document length at which the fan-out
/// beats a sequential loop — is **unmeasured** for this implementation, which
/// changed algorithm entirely in the Rust-native migration. Rayon's own task
/// scheduling costs on the order of a microsecond, and `remove_diacritics` on
/// a short document costs far less than that, so a plain
/// `inputs.iter().map(remove_diacritics).collect()` is the right default for a
/// handful of strings. Reach for this for corpus-scale batches, and measure
/// your own workload rather than assuming the win.
///
/// # Allocation
///
/// One `Vec<Cow<str>>` sized to `inputs.len()`, plus whatever
/// `remove_diacritics` allocates per input. No additional buffering, no
/// locking, and no per-call thread-pool construction — this uses whichever
/// global `rayon` pool is already installed, so pool configuration stays the
/// caller's business.
///
/// # Order
///
/// `results[i]` is `remove_diacritics(inputs[i])`, via `rayon`'s
/// order-preserving `map` + `collect`.
///
/// # Examples
///
/// ```
/// use verbora_normalizers::par_remove_diacritics_batch;
///
/// let inputs = ["café", "naïve", "ASCII only"];
/// assert_eq!(par_remove_diacritics_batch(&inputs), ["cafe", "naive", "ASCII only"]);
/// ```
#[cfg(feature = "parallel")]
#[cfg_attr(docsrs, doc(cfg(feature = "parallel")))]
#[must_use]
pub fn par_remove_diacritics_batch<'a>(inputs: &[&'a str]) -> Vec<Cow<'a, str>> {
    use rayon::prelude::*;
    inputs.par_iter().map(|s| remove_diacritics(s)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_and_ascii_borrow() {
        assert!(matches!(remove_diacritics(""), Cow::Borrowed(_)));
        assert!(matches!(remove_diacritics("ABC abc 123"), Cow::Borrowed(_)));
    }

    /// Derivations, every one from `UnicodeData.txt` field 5
    /// (`Decomposition_Mapping`) and field 3 (`Canonical_Combining_Class`).
    #[test]
    fn latin_folds() {
        // U+00E9 -> 0065 0301, ccc(0301) = 230.
        assert_eq!(remove_diacritics("café"), "cafe");
        // U+00EF -> 0069 0308, ccc(0308) = 230.
        assert_eq!(remove_diacritics("naïve"), "naive");
        // U+00E5 -> 0061 030A; U+00F6 -> 006F 0308.
        assert_eq!(remove_diacritics("Ångström"), "Angstrom");
        // U+00E7 -> 0063 0327, ccc(0327) = 202; U+00E3 -> 0061 0303.
        assert_eq!(remove_diacritics("coração"), "coracao");
    }

    /// The `Cow` iff-guarantee, over the pair an ASCII-only test cannot
    /// separate: a precomposed string that is already NFC but whose NFD holds
    /// a mark.
    #[test]
    fn cow_is_borrowed_exactly_when_nothing_changed() {
        for s in ["", "abc", "Москва", "日本語", "한국", "ก", "😀", "ß", "ø"] {
            let got = remove_diacritics(s);
            assert!(
                matches!(got, Cow::Borrowed(_)),
                "{s:?} should have been borrowed, got {got:?}"
            );
            assert_eq!(got, s);
        }
        for s in ["café", "cafe\u{0301}", "\u{1D167}", "שָׁ"] {
            let got = remove_diacritics(s);
            assert!(
                matches!(got, Cow::Owned(_)),
                "{s:?} should have been owned, got {got:?}"
            );
            assert_ne!(got.as_ref(), s);
        }
    }

    /// A string in NFD with no marks in it at all is still not borrowed if it
    /// is not in NFC — the guarantee is about equality with the *input*, and
    /// the function's last step is composition.
    #[test]
    fn decomposed_hangul_is_recomposed_and_therefore_owned() {
        let jamo = "\u{1112}\u{1161}\u{11AB}";
        let got = remove_diacritics(jamo);
        assert_eq!(got, "한");
        assert!(matches!(got, Cow::Owned(_)));
    }
}
