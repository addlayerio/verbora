//! The per-language gate predicates: which characters make a token worth
//! stemming.
//!
//! Checked-in data, one predicate per language, each a set of UTF-16 code units
//! collapsed into ranges over the Basic Multilingual Plane. The interface being
//! in UTF-16 code units is an open item of the stemmers' own migration rather
//! than settled contract — the text-shaping contract removes UTF-16 everywhere
//! else — but every set below is now stated as *an alphabet*, with the
//! language it belongs to named, so a reader can check it against something.
//!
//! # What a gate decides
//!
//! A gate is applied with `.any()` in [`TokenizeAndStem::gate`], so a token
//! passes as soon as **one** of its scalars is in the set, and a token that
//! fails is emitted **unstemmed** — never dropped, never split. The question a
//! gate answers is therefore "is this token made of this language's letters at
//! all?", and a `false` means "leave it alone", not "discard it" and not
//! "break here".
//!
//! Two consequences follow, and both matter when judging one of these sets:
//!
//! * **`.any()` makes an omission quiet.** Any word holding one a–z letter
//!   passes whatever else is missing from its language's set, so an omitted
//!   accented letter changes nothing for real words — it shows up only for a
//!   token made *entirely* of the omitted letters, and through the public
//!   [`TokenizeAndStem::gate`] itself.
//! * **Only the lower-case half is reachable through the pipeline.** All seven
//!   gated languages set `STEM_ON = Casing::Lower` (pinned by
//!   `audit::the_filter_casing_of_every_language_is_the_documented_one` for the
//!   filter axis and by [`crate::base`]'s table for this one), and the gate is
//!   applied to the string `STEM_ON` selects. The upper-case code units are
//!   reachable only by calling `TokenizeAndStem::gate` directly, which is
//!   public, so they are kept and stated.
//!
//! # Coverage against each language's own alphabet
//!
//! `tests::every_stop_word_passes_its_own_language_gate` walks every entry of
//! all seven lists through the gate that guards it and pins the exact set each
//! one rejects, so no entry can go quietly ungated. That test says nothing
//! about *omissions*, though, because `.any()` hides them — so the table below
//! records what was found by comparing each set against the alphabet it claims
//! to be, character by character:
//!
//! | gate | complete for its language? |
//! |---|---|
//! | [`gate_de`] | **was a verbatim copy of [`gate_es`]**; corrected here |
//! | [`gate_es`] | yes — `á é í ñ ó ú ü` and their capitals, all present |
//! | [`gate_ru`] | yes — `А`–`я` plus `Ё ё` |
//! | [`gate_uk`] | yes — `Є І Ї Ґ` and their lower case, over the shared range |
//! | [`gate_fr`] | no — omits `ä ö ü ÿ æ œ` and their capitals |
//! | [`gate_it`] | no — omits `é í î ó ú` and their capitals |
//! | [`gate_nl`] | no — omits `à â ê î ô û` and their capitals |
//!
//! The three incomplete sets are **not** the copy-paste [`gate_de`] was: each
//! holds only letters of its own language and merely stops short of the whole
//! alphabet. Correcting them is a separate change with its own derivation to
//! write down — `é` is everyday Italian (`perché`, `né`, `poiché`) while the
//! Dutch and French omissions are mostly loan-word diacritics — and none of
//! them is reachable through the pipeline for any word that also holds an a–z
//! letter, which every affected word does.
//!
//! [`TokenizeAndStem::gate`]: crate::TokenizeAndStem::gate

/// German: `A`–`Z`, `a`–`z`, the three umlauts, the Eszett, and ASCII digits.
///
/// # Basis
///
/// The German alphabet — the 26 basic Latin letters plus `Ä Ö Ü` / `ä ö ü` and
/// `ß`, whose capital `ẞ` (`U+1E9E`) the Rat für deutsche Rechtschreibung
/// admitted to the official orthography in its 2017 ruleset. The digits are
/// what every gate in this file admits and are kept for that reason.
///
/// [`crate::PorterStemmerDe`]'s own algorithm names exactly the same four
/// non-ASCII letters and no others, which is the derivation this set is checked
/// against rather than a bare character list: its prelude rewrites `ß` to `ss`,
/// its postlude folds `ä ö ü` to `a o u` (unless `preserve_umlauts` is set),
/// and every suffix table it consults is ASCII. No rule anywhere in that
/// stemmer is keyed on any other letter.
///
/// `é` stays out deliberately. It occurs in German only in unassimilated
/// loanwords (`Café`, `Attaché`, `Varieté`), belongs to no German word's
/// spelling, and admitting it would be a smaller version of the defect below;
/// every such loanword holds a–z letters and passes on those regardless.
///
/// # What this replaced
///
/// This predicate was a **byte-for-byte copy of [`gate_es`]**. It admitted
/// `Á É Í Ñ Ó Ú á é í ñ ó ú` — letters no German word is spelled with — and
/// omitted `Ä Ö ä ö ß`, four letters that are. `Ü ü` were present only
/// incidentally, because Spanish spells `pingüino` with one.
///
/// The consequence for real German text was nil, and that is measured rather
/// than assumed: a gate is `.any()`, and all **620** entries of the German
/// stop-word list — including the **52** that carry `ä ö ü ß` (23 `ü`, 13 `ß`,
/// 10 `ä`, 8 `ö`) — hold at least one a–z letter, as does every word of the
/// crate's German bench fixture. `tests::the_corrected_german_gate_changes_no_
/// german_word` walks all of them through both the old and the new predicate
/// and pins that the two agree on every single one.
///
/// What did change is the answer a caller gets from the public
/// `PorterStemmerDe::gate`: `gate("ö")` was `false` and is now `true`;
/// `gate("ñ")` was `true` and is now `false`.
#[inline]
pub(crate) fn gate_de(unit: u16) -> bool {
    matches!(unit,
        48..=57         // 0-9
            | 65..=90   // A-Z
            | 97..=122  // a-z
            | 196       // Ä
            | 214       // Ö
            | 220       // Ü
            | 223       // ß
            | 228       // ä
            | 246       // ö
            | 252       // ü
            | 7838      // ẞ  (U+1E9E)
    )
}

/// Spanish: `A`–`Z`, `a`–`z`, `á é í ñ ó ú ü` with their capitals, and digits.
///
/// Complete for its language: the Spanish alphabet's only letter outside the
/// basic Latin 26 is `ñ`, and the only diacritics its orthography uses are the
/// acute accent on the five vowels and the diaeresis on `u` (`pingüino`). All
/// fourteen code units are present.
#[inline]
pub(crate) fn gate_es(unit: u16) -> bool {
    matches!(unit,
        48..=57
            |         65..=90
            |         97..=122
            |         193
            |         201
            |         205
            |         209
            |         211
            |         218
            |         220
            |         225
            |         233
            |         237
            |         241
            |         243
            |         250
            |         252
    )
}

/// French, shared by [`crate::PorterStemmerFr`] and [`crate::CarryStemmerFr`].
///
/// **Incomplete for its language**, in its own letters rather than another's:
/// it admits `à â ç è é ê ë î ï ô ù û` and their capitals but omits
/// `ä ö ü ÿ æ œ` and theirs — the diaeresis vowels of `Staël`, `Noël`,
/// `aiguë` and `L'Haÿ`, and the two ligatures. Sharing the set between the two
/// French stemmers is deliberate: they stem the same language.
#[inline]
pub(crate) fn gate_fr(unit: u16) -> bool {
    matches!(unit,
        48..=57
            |         65..=90
            |         97..=122
            |         192
            |         194
            |         199..=203
            |         206..=207
            |         212
            |         217
            |         219
            |         224
            |         226
            |         231..=235
            |         238..=239
            |         244
            |         249
            |         251
    )
}

/// Italian: `A`–`Z`, `a`–`z`, the grave vowels `à è ì ò ù`, and digits.
///
/// **Incomplete for its language.** Italian's acute `é` is not a loan-word
/// diacritic but everyday orthography — `perché`, `né`, `poiché`, `affinché` —
/// and it is omitted here, as are `í î ó ú` and every capital of the five.
/// Nothing observable follows for those words, because each also holds a–z
/// letters and the gate is `.any()`; correcting the set is a separate change.
#[inline]
pub(crate) fn gate_it(unit: u16) -> bool {
    matches!(unit,
        48..=57
            |         65..=90
            |         97..=122
            |         192
            |         200
            |         204
            |         210
            |         217
            |         224
            |         232
            |         236
            |         242
            |         249
    )
}

/// Dutch: `A`–`Z`, `a`–`z`, the acute and diaeresis vowels, and digits.
///
/// Admits `á é í ó ú` (the *klemtoonteken*) and `ä ë ï ö ü` (the *trema*),
/// plus `è` and their capitals. **Incomplete**: the circumflex and grave
/// vowels `â ê î ô û à` of loanwords such as `enquête`, `crème` and `à la`
/// are omitted, along with their capitals.
#[inline]
pub(crate) fn gate_nl(unit: u16) -> bool {
    matches!(unit,
        48..=57
            |         65..=90
            |         97..=122
            |         193
            |         196
            |         200..=201
            |         203
            |         205
            |         207
            |         211
            |         214
            |         218
            |         220
            |         225
            |         228
            |         232..=233
            |         235
            |         237
            |         239
            |         243
            |         246
            |         250
            |         252
    )
}

/// Russian: `А`–`я` (`U+0410`–`U+044F`), `Ё ё`, digits, and the historic
/// Cyrillic small letters `U+1C80`–`U+1C86`.
///
/// Complete for its language: the 33-letter Russian alphabet is the contiguous
/// `U+0410`–`U+044F` block plus `Ё`/`ё`, which sit outside it, and all are
/// present. Latin letters are deliberately absent — a Latin token in Russian
/// text is not a word the Russian suffix rules were written for, so it is
/// emitted unstemmed. The `U+1C80`–`U+1C86` range over-admits Old Church
/// Slavonic letter variants that modern Russian does not use; harmless, since
/// a gate only ever grants stemming to a token that would otherwise pass
/// through.
#[inline]
pub(crate) fn gate_ru(unit: u16) -> bool {
    matches!(unit,
        48..=57
            |         1025
            |         1040..=1103
            |         1105
            |         7296..=7302
    )
}

/// Ukrainian: the shared Cyrillic range plus `Є І Ї Ґ` and their lower case.
///
/// Complete for its language: Ukrainian's four letters outside the
/// `U+0410`–`U+044F` block are `Є`/`є`, `І`/`і`, `Ї`/`ї` and `Ґ`/`ґ`, and all
/// eight are named. Reusing the whole shared range over-admits the four
/// Russian-only letters `ы ъ э ё`, which is harmless for the same reason
/// [`gate_ru`]'s over-admission is.
#[inline]
pub(crate) fn gate_uk(unit: u16) -> bool {
    matches!(unit,
        48..=57
            |         1028
            |         1030..=1031
            |         1040..=1103
            |         1108
            |         1110..=1111
            |         1168..=1169
            |         7296..=7302
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::base::{Casing, TokenizeAndStem};
    use crate::data::audit::str_slice_literal;
    use crate::stopwords::Language;
    use crate::{
        CarryStemmerFr, PorterStemmerDe, PorterStemmerEs, PorterStemmerFr, PorterStemmerIt,
        PorterStemmerNl, PorterStemmerRu, PorterStemmerUk,
    };

    /// `benches/stemmers.rs`, read at compile time so its per-language word
    /// fixtures are audited rather than transcribed.
    ///
    /// A copy of `WORDS_DE` pasted into this file would be a second table that
    /// nothing keeps in step with the first — the exact failure
    /// `audit::the_second_copy_of_every_list_still_matches_this_one` exists to
    /// catch elsewhere. The `include_str!` is `#[cfg(test)]`-only, so it does
    /// not participate in a packaged build.
    const BENCH_SOURCE: &str = include_str!("../../benches/stemmers.rs");

    fn bench_words(code: &str) -> Vec<String> {
        str_slice_literal(BENCH_SOURCE, &format!("const WORDS_{code}: &[&str] = &["))
    }

    /// The gate verdict the pipeline actually reaches, for one entry.
    ///
    /// [`crate::base::Stems::next`] applies the gate to `pick(STEM_ON)`, so
    /// asking `S::gate(entry)` directly would be asking a different question
    /// for every language that stems on the lowercased token — which, as the
    /// module note records, is all seven of the gated ones. Reading `STEM_ON`
    /// here rather than lowercasing unconditionally keeps this a mirror of the
    /// pipeline instead of a second opinion about it.
    fn gate_through_pipeline<S: TokenizeAndStem>(entry: &str) -> bool {
        let lowered;
        let input = match S::STEM_ON {
            Casing::Raw => entry,
            Casing::Lower => {
                lowered = entry.to_lowercase();
                &lowered
            }
        };
        S::gate(input)
    }

    /// Whether one character, on its own, passes `gate`.
    ///
    /// A gate takes UTF-16 code units, so a scalar outside the BMP arrives as
    /// two surrogates; `.any()` over both is what the pipeline does.
    fn admits(gate: fn(u16) -> bool, c: char) -> bool {
        let mut buf = [0u16; 2];
        c.encode_utf16(&mut buf).iter().copied().any(gate)
    }

    /// `gate_de` before the repair: a byte-for-byte copy of [`gate_es`], kept
    /// verbatim so the two predicates can be compared over a real corpus.
    fn gate_de_before_the_fix(unit: u16) -> bool {
        matches!(unit,
            48..=57
                | 65..=90
                | 97..=122
                | 193 | 201 | 205 | 209 | 211 | 218 | 220
                | 225 | 233 | 237 | 241 | 243 | 250 | 252
        )
    }

    /// Every letter of the German alphabet, on its own, passes the German gate.
    ///
    /// One character per case is the whole point: `.any()` means a letter that
    /// is missing from the set is invisible in any word that also holds an a–z
    /// letter, so a corpus-level test cannot see the omission at all. Asking
    /// about the letter by itself is the only question whose answer changes.
    ///
    /// This is the assertion that failed before the repair — `ä`, `ö`, `ß` and
    /// their capitals were all rejected — and it enumerates the alphabet rather
    /// than sampling it.
    #[test]
    fn every_letter_of_the_german_alphabet_passes_the_german_gate() {
        // The 26 basic letters in both cases, the umlauts, the Eszett with its
        // 2017 capital, and the digits every gate in this file admits.
        let written_with: Vec<char> = ('a'..='z')
            .chain('A'..='Z')
            .chain("äöüßÄÖÜẞ".chars())
            .chain('0'..='9')
            .collect();
        assert_eq!(written_with.len(), 26 + 26 + 8 + 10);
        let rejected: Vec<char> = written_with
            .into_iter()
            .filter(|c| !admits(gate_de, *c))
            .collect();
        assert!(
            rejected.is_empty(),
            "the German gate rejects {} characters German is written with: {rejected:?}",
            rejected.len()
        );
    }

    /// ...and the letters it used to hold instead are gone.
    ///
    /// `Á É Í Ñ Ó Ú` and their lower case are Spanish, not German; they were
    /// admitted only because this predicate was a copy of [`gate_es`]. `ü` is
    /// deliberately absent from this list — Spanish spells `pingüino` with one
    /// and German spells `für` with one, so it belongs to both sets and was
    /// correct by coincidence.
    #[test]
    fn the_german_gate_no_longer_holds_the_spanish_letters() {
        for c in "áéíñóúÁÉÍÑÓÚ".chars() {
            assert!(
                !admits(gate_de, c),
                "the German gate still admits the Spanish letter {c:?}"
            );
            assert!(admits(gate_es, c), "{c:?} should still be Spanish");
        }
    }

    /// The repair changes the verdict on no German word — enumerated, not
    /// sampled.
    ///
    /// Every entry of the 620-word German stop-word list and every word of the
    /// crate's German bench fixture goes through the pipeline's own gate call
    /// under both the old predicate and the new one. All of them pass both,
    /// because a gate is `.any()` and every German word holds an a–z letter;
    /// 58 of the 636 carry `ä ö ü ß` — 52 of the 620 stop words (23 `ü`,
    /// 13 `ß`, 10 `ä`, 8 `ö`) and 6 of the 16 bench words — and every one of
    /// them was stemmed anyway, on its unaccented letters.
    ///
    /// That is the measurement behind the claim on [`gate_de`] that the wrong
    /// set was wrong rather than harmful. It is also the tripwire for the
    /// opposite mistake: a future edit that narrows the set enough to start
    /// exempting real German words fails here.
    #[test]
    fn the_corrected_german_gate_changes_no_german_word() {
        let bench = bench_words("DE");
        let corpus: Vec<&str> = Language::De
            .defaults()
            .iter()
            .copied()
            .chain(bench.iter().map(String::as_str))
            .collect();
        assert_eq!(
            corpus.len(),
            620 + 16,
            "the German corpus this test enumerates changed size"
        );

        let mut with_umlauts = 0;
        for entry in corpus {
            let lowered = entry.to_lowercase();
            let before = lowered.encode_utf16().any(gate_de_before_the_fix);
            let after = gate_through_pipeline::<PorterStemmerDe>(entry);
            assert_eq!(
                before, after,
                "the corrected German gate changed its verdict on {entry:?}"
            );
            assert!(after, "{entry:?} is a German word the German gate rejects");
            if lowered.chars().any(|c| "äöüß".contains(c)) {
                with_umlauts += 1;
            }
        }
        assert_eq!(
            with_umlauts, 58,
            "the number of German corpus words spelled with ä ö ü ß changed"
        );
    }

    /// Every entry of every gated language's stop-word list, through the gate
    /// that guards it, with the exact rejected set pinned per language.
    ///
    /// A rejected entry is not a defect — the gate emits it unstemmed, which
    /// is right for something that holds none of the language's letters — but
    /// the *set* of them is a fact about the data, and pinning it by equality
    /// means neither a gate edit nor a list edit can change which entries are
    /// exempt without saying so here.
    ///
    /// Two rejections are worth naming. Latin `c` and `o` sit in the Russian
    /// list and Latin `o` in the Ukrainian one, alongside their Cyrillic twins
    /// `с` and `о`, which are also listed; both spellings are live stop words,
    /// so nothing is dead, but only the Cyrillic pair is a Cyrillic word and
    /// only that pair passes the gate. The rest are `$`, `_` and `-`, which
    /// hold no letter in any script — `audit` already records them as entries
    /// no token can equal.
    #[test]
    fn every_stop_word_passes_its_own_language_gate() {
        fn check<S: TokenizeAndStem>(code: &str, lang: Language, expected: &[&str]) {
            let rejected: Vec<&str> = lang
                .defaults()
                .iter()
                .copied()
                .filter(|entry| !gate_through_pipeline::<S>(entry))
                .collect();
            assert_eq!(
                rejected, expected,
                "{code}: the set of stop words its own gate exempts from stemming changed"
            );
        }

        check::<PorterStemmerDe>("de", Language::De, &[]);
        check::<PorterStemmerEs>("es", Language::Es, &["_"]);
        check::<PorterStemmerFr>("fr", Language::Fr, &[]);
        check::<CarryStemmerFr>("fr (Carry)", Language::Fr, &[]);
        check::<PorterStemmerIt>("it", Language::It, &["_"]);
        check::<PorterStemmerNl>("nl", Language::Nl, &["$", "_", "-"]);
        check::<PorterStemmerRu>("ru", Language::Ru, &["c", "o", "$", "_"]);
        check::<PorterStemmerUk>("uk", Language::Uk, &["o", "$", "_"]);
    }

    /// The gate is a stemming exemption, not a filter and not a boundary.
    ///
    /// [`crate::base::Stems::next`] emits a gate-failing token unchanged in the
    /// `STEM_ON` casing, so the token stream keeps its length either way. Pinned
    /// because the repair above only makes sense under this reading: had a
    /// `false` meant "drop", widening the German set would have changed which
    /// tokens survive rather than which get stemmed.
    #[test]
    fn a_failed_gate_emits_the_token_unstemmed_rather_than_dropping_it() {
        let ru = PorterStemmerRu::new();
        // `keep_stops` so the stop-word list cannot hide the effect: Latin
        // "Computer" holds no Cyrillic letter and fails `gate_ru`.
        assert!(!gate_through_pipeline::<PorterStemmerRu>("Computer"));
        let out = ru.tokenize_and_stem("Computer важнейшими", true);
        assert_eq!(out.len(), 2, "a gate-failing token must still be emitted");
        assert_eq!(
            out[0], "computer",
            "it must come back unstemmed, in the STEM_ON casing"
        );
    }
}
