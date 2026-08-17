//! The Spanish Snowball stemmer, ported from
//! The reference `porter_stemmer_es`.
//!
//! # `stem` does not lowercase
//!
//! Line 99 of the reference is `word.toLowerCase()` — a statement whose result is
//! thrown away. Because `isVowel` carries `/i` while every suffix comparison is
//! case-sensitive, uppercase input flows through the region machinery and then
//! matches nothing: `stem("ÁRBOL")` is `"ÁRBOL"`, `stem("Efecto")` is `"Efect"`,
//! `stem("campa")` is `"camp"`. Adding the "obviously missing" fold changes
//! results for every caller that reaches `stem` directly rather than through
//! `tokenizeAndStem` (which folds separately).
//!
//! # Two more traps
//!
//! `removeAccent` calls `String.prototype.replace` with a **string** pattern, so
//! it rewrites only the first occurrence of each accented vowel:
//! `removeAccent("ááéé")` is `"aáeé"`. Rust's `str::replace` replaces all.
//!
//! The step-2b verb list contains the entry `"  aseis"` — with two leading
//! spaces. It is dead as written, and it is preserved verbatim: "fixing" it to
//! `"aseis"` would start matching real words.

use std::borrow::Cow;

use verbora_tokenizers::classes;

use crate::base::{Casing, TokenizeAndStem};
use crate::data::charsets::is_es_vowel;
use crate::data::gates::gate_es;
use crate::stopwords::{self, Language};
use crate::units::{ends_with, longest_suffix, slen, text, u, units};

/// The Spanish Snowball stemmer.
///
/// ```
/// use verbora_stemmers::PorterStemmerEs;
/// let s = PorterStemmerEs::new();
/// assert_eq!(s.stem("campa"), "camp");
/// // Uppercase input is returned essentially unchanged — see the module docs.
/// assert_eq!(s.stem("CAMPA"), "CAMPA");
/// ```
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PorterStemmerEs;

#[inline]
fn is_vowel(c: u16) -> bool {
    is_es_vowel(c)
}

/// The slice of `w` from `at`, clamped — the reference's `slice` never panics, and
/// the region indices are computed once and reused after the word has shrunk.
#[inline]
fn from(w: &[u16], at: usize) -> &[u16] {
    &w[at.min(w.len())..]
}

/// `word.slice(0, -n)`, with the same clamping.
#[inline]
fn drop_last(w: &[u16], n: usize) -> Vec<u16> {
    w[..w.len().saturating_sub(n)].to_vec()
}

impl PorterStemmerEs {
    /// Creates the stemmer. It is stateless and zero-sized.
    #[inline]
    pub const fn new() -> Self {
        Self
    }

    /// `isVowel` — case-insensitive, which is why uppercase words still get
    /// regions.
    #[allow(
        clippy::unused_self,
        reason = "mirrors the reference's method-shaped API"
    )]
    pub fn is_vowel(&self, c: &str) -> bool {
        c.encode_utf16().any(is_vowel)
    }

    /// The index of the next vowel at or after `start`, or the length.
    #[allow(
        clippy::unused_self,
        reason = "mirrors the reference's method-shaped API"
    )]
    pub fn next_vowel_position(&self, word: &str, start: usize) -> usize {
        let w = units(word);
        (start..w.len())
            .find(|&i| is_vowel(w[i]))
            .unwrap_or(w.len())
    }

    /// The index of the next consonant at or after `start`, or the length.
    #[allow(
        clippy::unused_self,
        reason = "mirrors the reference's method-shaped API"
    )]
    pub fn next_consonant_position(&self, word: &str, start: usize) -> usize {
        let w = units(word);
        (start..w.len())
            .find(|&i| !is_vowel(w[i]))
            .unwrap_or(w.len())
    }

    /// Whether `word` ends with `suffix`, guarding on length as the reference does.
    #[allow(
        clippy::unused_self,
        reason = "mirrors the reference's method-shaped API"
    )]
    pub fn ends_in(&self, word: &str, suffix: &str) -> bool {
        slen(word) >= slen(suffix) && ends_with(&units(word), suffix)
    }

    /// The **longest** matching suffix, or `""`.
    ///
    /// Spanish sorts its matches by length; Italian and Portuguese take the first
    /// in array order instead. The two policies are not interchangeable.
    #[allow(
        clippy::unused_self,
        reason = "mirrors the reference's method-shaped API"
    )]
    pub fn ends_in_arr<'s>(&self, word: &str, suffixes: &[&'s str]) -> &'s str {
        longest_suffix(&units(word), suffixes).unwrap_or("")
    }

    /// Replaces the **first** occurrence of each accented vowel, in the order
    /// á é í ó ú.
    #[allow(
        clippy::unused_self,
        reason = "mirrors the reference's method-shaped API"
    )]
    pub fn remove_accent<'a>(&self, word: &'a str) -> Cow<'a, str> {
        if !word
            .chars()
            .any(|c| matches!(c, 'á' | 'é' | 'í' | 'ó' | 'ú'))
        {
            return Cow::Borrowed(word);
        }
        let mut out = word.to_owned();
        for (accented, plain) in [('á', 'a'), ('é', 'e'), ('í', 'i'), ('ó', 'o'), ('ú', 'u')] {
            if let Some(idx) = out.find(accented) {
                out.replace_range(
                    idx..idx + accented.len_utf8(),
                    plain.encode_utf8(&mut [0; 4]),
                );
            }
        }
        Cow::Owned(out)
    }

    fn remove_accent_units(w: &[u16]) -> Vec<u16> {
        let mut out = w.to_vec();
        for (accented, plain) in [
            (u('á'), u('a')),
            (u('é'), u('e')),
            (u('í'), u('i')),
            (u('ó'), u('o')),
            (u('ú'), u('u')),
        ] {
            if let Some(i) = out.iter().position(|&c| c == accented) {
                out[i] = plain;
            }
        }
        out
    }

    /// Stems one token.
    #[allow(
        clippy::unused_self,
        reason = "mirrors the reference's method-shaped API"
    )]
    #[expect(
        clippy::too_many_lines,
        reason = "one else-if chain per Snowball step; splitting it would obscure the order, which is the specification"
    )]
    pub fn stem<'a>(&self, word: &'a str) -> Cow<'a, str> {
        let mut w = units(word);
        let length = w.len();
        if length < 2 {
            return Cow::Owned(text(&Self::remove_accent_units(&w)));
        }

        let (mut r1, mut r2, mut rv) = (length, length, length);
        for i in 0..length - 1 {
            if r1 != length {
                break;
            }
            if is_vowel(w[i]) && !is_vowel(w[i + 1]) {
                r1 = i + 2;
            }
        }
        for i in r1..length.saturating_sub(1) {
            if r2 != length {
                break;
            }
            if is_vowel(w[i]) && !is_vowel(w[i + 1]) {
                r2 = i + 2;
            }
        }
        if length > 3 {
            if !is_vowel(w[1]) {
                rv = (2..length).find(|&i| is_vowel(w[i])).unwrap_or(length) + 1;
            } else if is_vowel(w[0]) && is_vowel(w[1]) {
                rv = (2..length).find(|&i| !is_vowel(w[i])).unwrap_or(length) + 1;
            } else {
                rv = 3;
            }
        }

        // --- Step 0: attached pronoun --------------------------------------
        //
        // The region slices are taken from the word as it stands *before* this
        // step, and refreshed afterwards only if the step changed something —
        // which, since they are recomputed from the same offsets, comes to the
        // same thing as refreshing unconditionally.
        if let Some(suffix) = longest_suffix(&w, PRONOUN) {
            let n = slen(suffix);
            let rv_text = from(&w, rv).to_vec();
            let head = &rv_text[..rv_text.len().saturating_sub(n)];
            if longest_suffix(head, PRONOUN_PRE1).is_some() {
                w = Self::remove_accent_units(&drop_last(&w, n));
            } else {
                let stem_head = drop_last(&w, n);
                if longest_suffix(head, PRONOUN_PRE2).is_some() || ends_with(&stem_head, "uyendo") {
                    w = stem_head;
                }
            }
        }

        // `r1Text`/`r2Text`/`rvText` are borrowed slices of `w` here, not owned
        // snapshots: `longest_suffix` returns `Option<&'s str>` borrowed from
        // the *suffix list* argument (`alts`), never from `word` — so the
        // borrow of `w` needed to call it ends the moment the call returns,
        // well before any of the branches below reassign `w`. Recomputing
        // `&w[r..]` fresh at each step (instead of cloning it once into a
        // `Vec<u16>`) is therefore the same bytes, just without the copy.
        // Protected by the full recorded-fixture parity suite
        // (`tests/parity.rs`), which replays real the reference output through
        // this function — not just the hand-written vectors below.
        let step1_start = w.clone();

        // --- Step 1: standard suffixes -------------------------------------
        if let Some(s) = longest_suffix(from(&w, r2), STEP1_A) {
            w = drop_last(&w, slen(s));
        } else if let Some(s) = longest_suffix(from(&w, r2), STEP1_B) {
            w = drop_last(&w, slen(s));
        } else if let Some(s) = longest_suffix(from(&w, r2), &["logía", "logías"]) {
            w = drop_last(&w, slen(s));
            w.extend("log".encode_utf16());
        } else if let Some(s) = longest_suffix(from(&w, r2), &["ución", "uciones"]) {
            w = drop_last(&w, slen(s));
            w.push(u('u'));
        } else if let Some(s) = longest_suffix(from(&w, r2), &["encia", "encias"]) {
            w = drop_last(&w, slen(s));
            w.extend("ente".encode_utf16());
        } else if let Some(s) = longest_suffix(
            from(&w, r2),
            &["ativamente", "ivamente", "osamente", "icamente", "adamente"],
        ) {
            w = drop_last(&w, slen(s));
        } else if let Some(s) = longest_suffix(from(&w, r1), &["amente"]) {
            w = drop_last(&w, slen(s));
        } else if let Some(s) = longest_suffix(
            from(&w, r2),
            &["antemente", "ablemente", "iblemente", "mente"],
        ) {
            w = drop_last(&w, slen(s));
        } else if let Some(s) = longest_suffix(
            from(&w, r2),
            &[
                "abilidad",
                "abilidades",
                "icidad",
                "icidades",
                "ividad",
                "ividades",
                "idad",
                "idades",
            ],
        ) {
            w = drop_last(&w, slen(s));
        } else if let Some(s) = longest_suffix(
            from(&w, r2),
            &[
                "ativa", "ativo", "ativas", "ativos", "iva", "ivo", "ivas", "ivos",
            ],
        ) {
            w = drop_last(&w, slen(s));
        }

        // `after0 == after1` in the reference's own shape is "did step 1
        // change anything" — a `bool` set by every mutating branch above is
        // the same fact without cloning the whole word to compare it.
        let step1_changed = w != step1_start;

        if !step1_changed {
            // --- Step 2a: `y` verb suffixes --------------------------------
            let mut step2a_changed = false;
            if let Some(s) = longest_suffix(from(&w, rv), STEP2A) {
                let n = slen(s);
                if w.len() > n && w[w.len() - n - 1] == u('u') {
                    w = drop_last(&w, n);
                    step2a_changed = true;
                }
            }

            // --- Step 2b: the rest of the verb suffixes --------------------
            if !step2a_changed {
                if let Some(s) = longest_suffix(from(&w, rv), STEP2B) {
                    w = drop_last(&w, slen(s));
                } else if let Some(s) = longest_suffix(from(&w, rv), &["en", "es", "éis", "emos"])
                {
                    w = drop_last(&w, slen(s));
                    if ends_with(&w, "gu") {
                        w.truncate(w.len() - 1);
                    }
                }
            }
        }

        // --- Step 3: residual ----------------------------------------------
        if let Some(s) = longest_suffix(from(&w, rv), &["os", "a", "o", "á", "í", "ó"]) {
            w = drop_last(&w, slen(s));
        } else if longest_suffix(from(&w, rv), &["e", "é"]).is_some() {
            w.truncate(w.len().saturating_sub(1));
            if ends_with(from(&w, rv), "u") && ends_with(&w, "gu") {
                w.truncate(w.len() - 1);
            }
        }

        Cow::Owned(text(&Self::remove_accent_units(&w)))
    }
}

/// Attached pronouns, matched against the whole word.
static PRONOUN: &[&str] = &[
    "me", "se", "sela", "selo", "selas", "selos", "la", "le", "lo", "las", "les", "los", "nos",
];
/// Accented gerund/infinitive endings that must precede a removed pronoun.
static PRONOUN_PRE1: &[&str] = &["iéndo", "ándo", "ár", "ér", "ír"];
/// The unaccented forms of the same.
static PRONOUN_PRE2: &[&str] = &["iendo", "ando", "ar", "er", "ir"];

static STEP1_A: &[&str] = &[
    "anza", "anzas", "ico", "ica", "icos", "icas", "ismo", "ismos", "able", "ables", "ible",
    "ibles", "ista", "istas", "oso", "osa", "osos", "osas", "amiento", "amientos", "imiento",
    "imientos",
];
static STEP1_B: &[&str] = &[
    "icadora",
    "icador",
    "icación",
    "icadoras",
    "icadores",
    "icaciones",
    "icante",
    "icantes",
    "icancia",
    "icancias",
    "adora",
    "ador",
    "ación",
    "adoras",
    "adores",
    "aciones",
    "ante",
    "antes",
    "ancia",
    "ancias",
];
static STEP2A: &[&str] = &[
    "ya", "ye", "yan", "yen", "yeron", "yendo", "yo", "yó", "yas", "yes", "yais", "yamos",
];
/// The step-2b verb list. `"  aseis"` really does carry two leading spaces.
static STEP2B: &[&str] = &[
    "arían", "arías", "arán", "arás", "aríais", "aría", "aréis", "aríamos", "aremos", "ará", "aré",
    "erían", "erías", "erán", "erás", "eríais", "ería", "eréis", "eríamos", "eremos", "erá", "eré",
    "irían", "irías", "irán", "irás", "iríais", "iría", "iréis", "iríamos", "iremos", "irá", "iré",
    "aba", "ada", "ida", "ía", "ara", "iera", "ad", "ed", "id", "ase", "iese", "aste", "iste",
    "an", "aban", "ían", "aran", "ieran", "asen", "iesen", "aron", "ieron", "ado", "ido", "ando",
    "iendo", "ió", "ar", "er", "ir", "as", "abas", "adas", "idas", "ías", "aras", "ieras", "ases",
    "ieses", "ís", "áis", "abais", "íais", "arais", "ierais", "  aseis", "ieseis", "asteis",
    "isteis", "ados", "idos", "amos", "ábamos", "íamos", "imos", "áramos", "iéramos", "iésemos",
    "ásemos",
];

impl TokenizeAndStem for PorterStemmerEs {
    const FILTER_ON: Casing = Casing::Raw;
    const STEM_ON: Casing = Casing::Lower;

    fn is_word_char(c: char) -> bool {
        classes::is_word_es(c)
    }

    fn is_stop_word(word: &str) -> bool {
        stopwords::contains(Language::Es, word)
    }

    fn gate(token: &str) -> bool {
        token.encode_utf16().any(gate_es)
    }

    fn stem_token(&self, token: &str) -> String {
        self.stem(token).into_owned()
    }
}

impl verbora_core::Stemmer for PorterStemmerEs {
    fn stem<'a>(&self, token: &'a str) -> Cow<'a, str> {
        Self::stem(self, token)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(t: &str) -> String {
        PorterStemmerEs::new().stem(t).into_owned()
    }

    #[test]
    fn uppercase_is_a_no_op() {
        assert_eq!(s("ÁRBOL"), "ÁRBOL");
        assert_eq!(s("CAMPA"), "CAMPA");
        assert_eq!(s("Efecto"), "Efect");
        assert_eq!(s("campa"), "camp");
    }

    #[test]
    fn remove_accent_hits_only_the_first_occurrence() {
        assert_eq!(PorterStemmerEs::new().remove_accent("ááéé"), "aáeé");
        assert_eq!(PorterStemmerEs::new().remove_accent("abc"), "abc");
    }

    #[test]
    fn edges_and_unicode() {
        assert_eq!(s(""), "");
        assert_eq!(s("a"), "a");
        assert_eq!(s("ab"), "ab");
        assert_eq!(s("á"), "a");
        assert_eq!(s("😀"), "😀");
        assert_eq!(s("日本語"), "日本語");
        assert_eq!(s("123"), "123");
    }
}
