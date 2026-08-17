//! `removeDiacritics` — the general Latin-script diacritic folder.
//!
//! Port of the reference `remove_diacritics`.

use std::borrow::Cow;

mod table;

/// Folds Latin diacritics to their base letters, exactly as the reference does.
///
/// # Why this is not Unicode normalization
///
/// It is tempting to implement this as NFD followed by stripping combining
/// marks, or to reach for a transliteration crate. Both give *different* answers,
/// in both directions:
///
/// * The shipped table has bugs that are part of the contract. `ſ` U+017F LATIN
///   SMALL LETTER LONG S folds to **`l`**, not `s`, because the original source
///   lists it inside the `l` character class. `ß` folds to `s` and `ẞ` to `S`
///   (not `ss`/`SS`). `İ` folds to `I` and `ı` to `i`.
/// * The table also *misses* things a correct implementation handles: it does not
///   decompose, so precomposed `é` U+00E9 folds to `e` while `e` + U+0301 passes
///   through untouched. Ligatures `ﬁ`/`ﬂ`, `Ĳ`/`ĳ`, `Ŋ`/`ŋ`, `ĸ`, `ȸ`/`ȹ` and
///   every non-BMP character are left alone.
///
/// Adding an NFD step or swapping in `deunicode` would break parity on real text.
///
/// # Why one pass instead of eighty-six
///
/// The reference runs 86 sequential global-regex passes over the whole string,
/// one per base letter. Every replacement it emits is an ASCII letter, and every
/// ASCII letter is itself a key of the table mapping to itself, so no pass can
/// ever cascade into another and the pass order is irrelevant. The whole thing is
/// therefore a single left-to-right per-character lookup — verified exhaustively
/// over all 63,488 non-surrogate BMP codepoints and 20,000 random strings when
/// the table was generated. This measures roughly twenty times faster on
/// document-scale input.
///
/// # Examples
///
/// ```
/// # use verbora_normalizers::remove_diacritics;
/// assert_eq!(remove_diacritics("piñon ça va über résumé œdipe"), "pinon ca va uber resume oedipe");
/// // Ill-advised but faithful: long s folds to `l`.
/// assert_eq!(remove_diacritics("ſ"), "l");
/// // No decomposition: the combining acute survives.
/// assert_eq!(remove_diacritics("e\u{0301}"), "e\u{0301}");
/// ```
#[must_use]
pub fn remove_diacritics(s: &str) -> Cow<'_, str> {
    // Every ASCII character is either absent from the table or maps to itself,
    // so pure-ASCII text — the overwhelmingly common case — is returned as-is
    // after one vectorised scan.
    if s.is_ascii() {
        return Cow::Borrowed(s);
    }

    let mut out: Option<String> = None;
    let mut copied = 0usize;

    for (i, c) in s.char_indices() {
        let Some(base) = fold(c) else { continue };
        let buf = out.get_or_insert_with(|| String::with_capacity(s.len()));
        buf.push_str(&s[copied..i]);
        buf.push_str(base);
        copied = i + c.len_utf8();
    }

    match out {
        Some(mut buf) => {
            buf.push_str(&s[copied..]);
            Cow::Owned(buf)
        }
        None => Cow::Borrowed(s),
    }
}

/// The base string for `c`, or `None` when the table leaves it alone.
///
/// The two-level bitmap in front of the binary search is what keeps this cheap
/// on text the table has nothing to say about: the 820 non-ASCII keys occupy
/// nine 256-codepoint blocks and are sparse inside several of them, so Cyrillic,
/// Greek, Hebrew, CJK, kana, halfwidth forms and emoji are all rejected in a
/// handful of operations rather than ten comparisons that always miss.
#[inline]
fn fold(c: char) -> Option<&'static str> {
    let cp = c as u32;
    // ASCII entries are identity and are omitted from the table; astral
    // characters have no entries at all.
    if !(0x80..=0xFFFF).contains(&cp) {
        return None;
    }
    let hi = (cp >> 8) as u8;
    if (table::BLOCKS[(hi >> 6) as usize] >> (hi & 63)) & 1 == 0 {
        return None;
    }
    let lo = (cp & 0xFF) as usize;
    let mut present = false;
    for &(block, ref bits) in table::STARTS {
        if block == hi {
            present = (bits[lo >> 6] >> (lo & 63)) & 1 != 0;
            break;
        }
        if block > hi {
            break; // `STARTS` is sorted by block.
        }
    }
    if !present {
        return None;
    }
    table::FOLD
        .binary_search_by_key(&c, |&(k, _)| k)
        .ok()
        .map(|i| table::FOLD[i].1)
}

/// [`remove_diacritics`], fanned out across a `rayon` thread pool. Requires
/// the `parallel` feature.
///
/// # Why this exists
///
/// `remove_diacritics` is a pure function of one `&str` with no shared state —
/// no allocation on the ASCII fast path, and the owned path only ever writes
/// into a buffer private to that call — so folding many independent documents
/// is embarrassingly parallel with zero coordination cost between them. This
/// function is exactly `inputs.par_iter().map(remove_diacritics).collect()` —
/// a thin fan-out over the existing sequential primitive, not a second
/// implementation of it. The per-character table lookup, the ASCII fast path
/// and the single-pass `Cow` construction inside `remove_diacritics` are all
/// untouched; if you need a different shape in parallel (a shared output
/// buffer, for instance), apply the same `par_iter().map(...)` pattern at
/// your own call site (see `site/performance/parallelism.md`).
///
/// # When to reach for it vs. the sequential loop
///
/// `remove_diacritics` is already cheap per call — this crate's own
/// `remove_diacritics` benchmark (`cargo bench -p verbora-normalizers --
/// remove_diacritics`) measures roughly 270 ns for a 128-byte accented
/// document up to roughly 35 µs for a 19 KB one, while a `rayon` task costs
/// on the order of a microsecond to schedule
/// (`site/performance/parallelism.md`). This crate's own
/// `par_remove_diacritics_batch` Criterion group (`benches/normalizers.rs`)
/// measures the consequence directly, at a fixed ~1.2 KB document per call: a
/// 16-document batch is roughly **1.5x slower** in parallel than the
/// sequential loop, while 256- and 4096-document batches are **5–8x faster**.
/// A plain `inputs.iter().map(remove_diacritics).collect()` loop wins for a
/// handful of documents; reach for this once the batch runs to hundreds of
/// documents or more, and measure your own workload rather than assuming the
/// win.
///
/// # Allocation behaviour
///
/// One `Vec<Cow<str>>` sized to `inputs.len()` for the output, plus whatever
/// `remove_diacritics` itself allocates per input — nothing for pure ASCII or
/// unmatched non-ASCII text (`Cow::Borrowed`), one `String` sized to the input
/// otherwise. No additional buffering, no locking, no per-call thread-pool
/// construction — this uses whichever global `rayon` pool is already
/// installed (or `rayon`'s default one), so pool configuration remains the
/// caller's responsibility, not this crate's.
///
/// # Order and errors
///
/// Output order matches input order — `results[i]` is
/// `remove_diacritics(inputs[i])` — via `rayon`'s order-preserving `map` +
/// `collect`. `remove_diacritics` never errors, so there is no error shape to
/// preserve.
///
/// # Examples
///
/// ```
/// use verbora_normalizers::diacritics::par_remove_diacritics_batch;
///
/// let inputs = ["café", "naïve", "ASCII only"];
/// let got = par_remove_diacritics_batch(&inputs);
/// assert_eq!(got, ["cafe", "naive", "ASCII only"]);
/// ```
#[cfg(feature = "parallel")]
#[must_use]
pub fn par_remove_diacritics_batch<'a>(inputs: &[&'a str]) -> Vec<Cow<'a, str>> {
    use rayon::prelude::*;
    inputs.par_iter().map(|s| remove_diacritics(s)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_and_single_characters() {
        assert_eq!(remove_diacritics(""), "");
        assert_eq!(remove_diacritics("a"), "a");
        assert_eq!(remove_diacritics("é"), "e");
    }

    #[test]
    fn ascii_is_returned_borrowed() {
        assert!(matches!(remove_diacritics("ABC abc 123"), Cow::Borrowed(_)));
        assert!(matches!(remove_diacritics(""), Cow::Borrowed(_)));
    }

    #[test]
    fn unmatched_non_ascii_is_returned_borrowed() {
        assert!(matches!(remove_diacritics("Москва"), Cow::Borrowed(_)));
        assert!(matches!(remove_diacritics("日本語"), Cow::Borrowed(_)));
    }

    #[test]
    fn all_uppercase_folds_to_uppercase() {
        assert_eq!(remove_diacritics("ÀÉÎÕÜ"), "AEIOU");
        assert_eq!(remove_diacritics("ÜBER"), "UBER");
    }

    #[test]
    fn accented_latin() {
        assert_eq!(remove_diacritics("café"), "cafe");
        assert_eq!(remove_diacritics("naïve"), "naive");
        assert_eq!(remove_diacritics("crème brûlée"), "creme brulee");
        assert_eq!(remove_diacritics("Ångström"), "Angstrom");
        assert_eq!(remove_diacritics("coração"), "coracao");
    }

    #[test]
    fn multi_character_bases() {
        assert_eq!(remove_diacritics("Æ æ Œ œ"), "AE ae OE oe");
        assert_eq!(remove_diacritics("Ǆǅǆ"), "DZDzdz");
        assert_eq!(remove_diacritics("ƕ"), "hv");
        assert_eq!(remove_diacritics("Ꜳ"), "AA");
    }

    #[test]
    fn shipped_quirks_are_reproduced() {
        // Long s is listed in the `l` class, not the `s` class.
        assert_eq!(remove_diacritics("ſ"), "l");
        assert_eq!(remove_diacritics("ẞ"), "S");
        assert_eq!(remove_diacritics("ß"), "s");
        assert_eq!(remove_diacritics("ŉ"), "n");
        assert_eq!(remove_diacritics("İı"), "Ii");
        assert_eq!(remove_diacritics("Ɐɐ"), "Aa");
        assert_eq!(remove_diacritics("ƒ"), "f");
    }

    #[test]
    fn characters_outside_the_table_pass_through() {
        // No decomposition, so a combining mark survives.
        assert_eq!(remove_diacritics("e\u{0301}"), "e\u{0301}");
        // Ligatures and letters the source never listed.
        assert_eq!(remove_diacritics("ﬁﬂĲĳŊŋĸȸȹ"), "ﬁﬂĲĳŊŋĸȸȹ");
    }

    #[test]
    fn other_scripts_are_untouched() {
        assert_eq!(remove_diacritics("Москва"), "Москва");
        assert_eq!(remove_diacritics("Ελλάδα"), "Ελλάδα");
        assert_eq!(remove_diacritics("日本語"), "日本語");
        assert_eq!(remove_diacritics("한국어"), "한국어");
    }

    #[test]
    fn astral_characters_are_untouched() {
        assert_eq!(remove_diacritics("😀"), "😀");
        assert_eq!(remove_diacritics("a😀é"), "a😀e");
        assert_eq!(remove_diacritics("𝕳𝖊𝖑𝖑𝖔"), "𝕳𝖊𝖑𝖑𝖔");
    }

    #[test]
    fn circled_and_fullwidth_forms_fold_to_plain_ascii() {
        assert_eq!(remove_diacritics("ⒶⓏⓐⓩ"), "AZaz");
        assert_eq!(remove_diacritics("ＡＺａｚ"), "AZaz");
    }

    #[test]
    fn punctuation_and_digits_are_untouched() {
        assert_eq!(remove_diacritics("¡¿«»…— 123"), "¡¿«»…— 123");
    }

    #[test]
    fn very_long_input() {
        let input = "é".repeat(10_000);
        assert_eq!(remove_diacritics(&input).len(), 10_000);
    }

    #[test]
    fn table_is_sorted_and_complete() {
        assert_eq!(
            table::FOLD.len(),
            872 - 52,
            "872 keys minus 52 ASCII identities"
        );
        assert!(table::FOLD.windows(2).all(|w| w[0].0 < w[1].0));
        assert!(table::STARTS.windows(2).all(|w| w[0].0 < w[1].0));
        // The gate is exact, so it must admit every key and nothing else. A
        // generated gate that is merely *nearly* right would show up as a silent
        // parity break rather than a compile error.
        assert!(table::FOLD.iter().all(|&(k, _)| fold(k).is_some()));
        let folded = (0x80u32..=0xFFFF)
            .filter_map(char::from_u32)
            .filter(|&c| fold(c).is_some())
            .count();
        assert_eq!(folded, table::FOLD.len());
    }

    /// Sequential-vs-parallel parity: `par_remove_diacritics_batch` must
    /// return exactly what a sequential `.iter().map(remove_diacritics)` loop
    /// returns, element for element, for every input this module's own
    /// sequential tests already exercise above — not a fresh set of edge
    /// cases.
    #[cfg(feature = "parallel")]
    mod parallel_parity {
        use super::*;

        /// Runs both paths over `inputs` and asserts they agree, item by item.
        fn assert_parity(inputs: &[&str]) {
            let sequential: Vec<Cow<'_, str>> =
                inputs.iter().map(|s| remove_diacritics(s)).collect();
            let parallel = par_remove_diacritics_batch(inputs);
            assert_eq!(
                parallel,
                sequential,
                "batch of {} inputs diverged from the sequential loop",
                inputs.len()
            );
        }

        #[test]
        fn empty_input_produces_an_empty_output() {
            assert_parity(&[]);
        }

        #[test]
        fn one_item() {
            assert_parity(&["café"]);
            assert_parity(&[""]);
            assert_parity(&["plain ascii"]);
        }

        #[test]
        fn many_items_preserve_order() {
            // The exact strings `remove_diacritics`'s own tests assert by
            // value above: ASCII, accented Latin, the shipped quirks (long s
            // -> l, ß -> s, dotted/dotless I), multi-character bases,
            // untouched scripts, astral characters, circled/fullwidth forms
            // and punctuation — cycled well past any plausible rayon task
            // count.
            let base = [
                "ABC abc 123",
                "café",
                "naïve",
                "crème brûlée",
                "Ångström",
                "coração",
                "Æ æ Œ œ",
                "Ǆǅǆ",
                "ƕ",
                "Ꜳ",
                "ſ",
                "ẞ",
                "ß",
                "ŉ",
                "İı",
                "Ɐɐ",
                "ƒ",
                "e\u{0301}",
                "ﬁﬂĲĳŊŋĸȸȹ",
                "Москва",
                "Ελλάδα",
                "日本語",
                "한국어",
                "😀",
                "a😀é",
                "𝕳𝖊𝖑𝖑𝖔",
                "ⒶⓏⓐⓩ",
                "ＡＺａｚ",
                "¡¿«»…— 123",
                "",
            ];
            let inputs: Vec<&str> = base.iter().copied().cycle().take(500).collect();
            assert_parity(&inputs);
        }

        #[test]
        fn unicode_and_pathological_inputs() {
            // The 10,000-character `remove_diacritics`'s own `very_long_input`
            // test exercises, alongside a mix of scripts this crate's own
            // suite treats as edge cases.
            let long = "é".repeat(10_000);
            let cyrillic = "Москва не сразу строилась ".repeat(40);
            let mixed = "piñon ça va über résumé œdipe ｱｲｳｴｵ 日本語".to_owned();
            let inputs: Vec<&str> = vec![long.as_str(), cyrillic.as_str(), mixed.as_str(), "é", ""];
            assert_parity(&inputs);
        }
    }
}
