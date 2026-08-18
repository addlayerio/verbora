//! `tokenizeAndStem`, expressed once.
//!
//! Every stemmer in the reference inherits a `tokenizeAndStem(text, keepStops)`
//! from its language's base class, and the thirteen base classes are almost —
//! but not quite — copies of one another. The differences are small, undocumented
//! and load-bearing:
//!
//! | | lowercase the text first | stop-word test reads | `stem()` receives | gate regex |
//! |---|---|---|---|---|
//! | en, id | **yes** | the token | the token | none |
//! | de, es, it, nl, ru, uk | no | the **raw** token | the lowercased token | yes |
//! | fr, Carry | no | the **lowercased** token | the lowercased token | yes |
//! | fa | no | the raw token | the raw token | none |
//! | no, sv, pt | no | the lowercased token | the **raw** token | none |
//! | ja | no | the raw token | the lowercased token | none |
//!
//! A port that picks one policy and applies it everywhere changes the token
//! stream for eight of the thirteen: German keeps a capitalised `"Das"` because
//! its stop-word list is consulted with the raw token, while lowercase `"ist"`,
//! `"und"` and `"die"` are dropped.
//!
//! The three axes — which text is tokenized, which string is filtered on, which
//! string is stemmed — are the only degrees of freedom, so [`TokenizeAndStem`]
//! names them and [`Stems`] implements all six variants once.
//!
//! # Laziness
//!
//! [`Stems`] is the primitive; `tokenize_and_stem` is `stems(..).collect()`. The
//! iterator holds the prepared text (borrowed when no rewrite was needed) and one
//! byte cursor, so a caller that only wants the first few stems of a large
//! document pays for only those.

use std::borrow::Cow;
use std::collections::HashMap;
use std::hash::BuildHasher;

/// Which form of a token a step looks at.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Casing {
    /// The token exactly as the tokenizer produced it.
    Raw,
    /// `token.toLowerCase()`.
    Lower,
}

/// The next maximal run of word characters in `s` at or after `from`.
///
/// The reference's aggressive tokenizers all reduce to this: `split` on a run of
/// non-word characters, then drop the empty strings the split leaves at the
/// edges. A maximal-run scan produces the same list without materialising the
/// separators, and — unlike `split` — without allocating.
fn next_run(s: &str, from: usize, is_word: fn(char) -> bool) -> Option<(usize, usize)> {
    let mut idx = from;
    let bytes = s.as_bytes();
    // Skip separators.
    while idx < s.len() {
        let c = next_char(s, idx);
        if is_word(c) {
            break;
        }
        idx += c.len_utf8();
    }
    if idx >= s.len() {
        return None;
    }
    let start = idx;
    while idx < bytes.len() {
        let c = next_char(s, idx);
        if !is_word(c) {
            break;
        }
        idx += c.len_utf8();
    }
    Some((start, idx))
}

#[inline]
fn next_char(s: &str, at: usize) -> char {
    s[at..].chars().next().expect("at is a char boundary")
}

/// A stemmer that can also tokenize text, following its language's recipe.
pub trait TokenizeAndStem {
    /// Which string the stop-word list is consulted with.
    const FILTER_ON: Casing;
    /// Which string is handed to `stem`.
    const STEM_ON: Casing;

    /// Whether [`Self::stem_token`] is a pure function of its input for this
    /// value — same token in, same stem out, with no observable state change.
    ///
    /// This is the gate for [`Self::tokenize_and_stem_cached`]: a cached stem
    /// is only a valid substitute for a fresh call when the fresh call could
    /// not have answered differently. Every stemmer in this crate is pure
    /// except [`crate::PorterStemmerNl`], whose sticky `suffix_e_removed`
    /// flag (a `Cell` written by step 2 and read by step 3b) makes both the
    /// output order-dependent *and* each call a state change that skipping
    /// would lose — it overrides this to `false`, and the cached entry point
    /// then ignores the cache entirely.
    ///
    /// Implementations with interior mutability in `stem_token` must override
    /// this to `false`. Mutation through `&mut self` *between* calls is the
    /// cache owner's concern, not this constant's: the caller who holds the
    /// cache must clear it when they reconfigure the stemmer it was filled by.
    const PURE_STEM: bool = true;

    /// The language's word-character class.
    fn is_word_char(c: char) -> bool;

    /// Rewrites the text before tokenizing.
    ///
    /// English and Indonesian lowercase the whole document here, which is *not*
    /// the same as lowercasing each token afterwards: `toLowerCase` can change a
    /// string's length (`'İ'` becomes `i` + U+0307, and U+0307 is a separator in
    /// every one of these classes), so it moves token boundaries. Norwegian and
    /// Swedish strip diacritics here instead. Everything else borrows unchanged.
    fn prepare(text: &str) -> Cow<'_, str> {
        Cow::Borrowed(text)
    }

    /// Whether `word` is currently a stop word for this language.
    fn is_stop_word(word: &str) -> bool;

    /// Whether the token contains a character that makes it worth stemming.
    ///
    /// A token that fails the gate is emitted *unstemmed* — in whichever casing
    /// [`Self::STEM_ON`] selects — rather than dropped.
    fn gate(_token: &str) -> bool {
        true
    }

    /// Stems one already-prepared token.
    fn stem_token(&self, token: &str) -> String;

    /// Lazily yields the stemmed tokens of `text`.
    ///
    /// This is the primitive; [`Self::tokenize_and_stem`] collects it.
    fn stems<'a>(&'a self, text: &'a str, keep_stops: bool) -> Stems<'a, Self>
    where
        Self: Sized,
    {
        Stems {
            stemmer: self,
            buf: Self::prepare(text),
            pos: 0,
            keep_stops,
        }
    }

    /// Tokenizes `text` and stems each token, dropping stop words unless
    /// `keep_stops`.
    fn tokenize_and_stem(&self, text: &str, keep_stops: bool) -> Vec<String>
    where
        Self: Sized,
    {
        self.stems(text, keep_stops).collect()
    }

    /// [`Self::tokenize_and_stem`] with a caller-owned `token → stem` cache.
    ///
    /// Stemming dominates repeated-text workloads — the classifier crate
    /// measured Porter `stem_token` calls at 65% of its whole Bayes training
    /// time, with the same few hundred distinct tokens re-stemmed thousands
    /// of times — so this entry point lets a caller that processes many
    /// documents pay for each distinct token once. The cache is passed in
    /// rather than held here, keeping every stemmer zero-sized and `Sync` and
    /// leaving eviction/lifetime policy (and the hasher) to the owner.
    ///
    /// # Parity contract
    ///
    /// The token stream, stop-word filtering and gate behaviour are exactly
    /// [`Self::tokenize_and_stem`]'s: the cache is consulted only for the
    /// string that would have been handed to [`Self::stem_token`], *after*
    /// the stop-word test (so mutations to the process-global stop-word lists
    /// keep taking effect between calls) and only for tokens that pass the
    /// gate (a gate-failing token is emitted unstemmed, never cached). Cache
    /// entries are trusted: hand the same map to two different stemmers and
    /// the second will happily serve the first one's stems.
    ///
    /// When [`Self::PURE_STEM`] is `false` the cache is ignored and this is
    /// exactly `tokenize_and_stem` — a stale answer from an impure stemmer
    /// would not merely be slow, it would be wrong.
    fn tokenize_and_stem_cached<H: BuildHasher>(
        &self,
        text: &str,
        keep_stops: bool,
        cache: &mut HashMap<String, String, H>,
    ) -> Vec<String>
    where
        Self: Sized,
    {
        if !Self::PURE_STEM {
            return self.tokenize_and_stem(text, keep_stops);
        }
        let buf = Self::prepare(text);
        let mut out = Vec::new();
        let mut pos = 0;
        // The same walk as `Stems::next`, with the `stem_token` call memoized.
        while let Some((start, end)) = next_run(&buf, pos, Self::is_word_char) {
            pos = end;
            let raw = &buf[start..end];
            let lowered: Option<String> =
                if Self::FILTER_ON == Casing::Lower || Self::STEM_ON == Casing::Lower {
                    Some(raw.to_lowercase())
                } else {
                    None
                };
            let pick = |casing: Casing| -> &str {
                match casing {
                    Casing::Raw => raw,
                    Casing::Lower => lowered.as_deref().unwrap_or(raw),
                }
            };
            if !keep_stops && Self::is_stop_word(pick(Self::FILTER_ON)) {
                continue;
            }
            let input = pick(Self::STEM_ON);
            out.push(if Self::gate(input) {
                match cache.get(input) {
                    Some(hit) => hit.clone(),
                    None => {
                        let stem = self.stem_token(input);
                        cache.insert(input.to_owned(), stem.clone());
                        stem
                    }
                }
            } else {
                input.to_owned()
            });
        }
        out
    }

    /// Runs [`Self::tokenize_and_stem`] over many independent documents, one
    /// rayon task per document, preserving input order.
    ///
    /// Requires the `parallel` feature.
    ///
    /// # Why per document, not per word
    ///
    /// This crate's own benchmarks (`stem-per-word` in `benches/stemmers.rs`)
    /// put a single [`Self::stem_token`] call anywhere from ~26 ns (Lancaster) to
    /// ~628 ns (Porter) to ~9 µs (the Indonesian dictionary lookup) — the fast
    /// end is at or below what it costs rayon merely to *schedule* a task,
    /// independent of what that task does. `words.par_iter().map(stem)` would
    /// spend most of its time in the work-stealing scheduler, not in stemming,
    /// and measurably regresses the fast stemmers rather than speeding them up.
    ///
    /// A whole document's tokenize-and-stem pipeline is a different story: it is
    /// the per-word cost times however many words the document holds, so a
    /// paragraph-sized document is comfortably above the scheduling floor. This
    /// method therefore fans out at the document boundary and runs each
    /// document's pipeline sequentially — unchanged — inside its task.
    ///
    /// `benches/stemmers.rs`'s `document-batch` group measures the crossover
    /// directly (English Porter, 32-core/32-thread machine, `cargo bench -p
    /// verbora-stemmers --features parallel -- document-batch`):
    ///
    /// | documents × words/doc | sequential | parallel | |
    /// |---|--:|--:|--:|
    /// | 4 × 16   | 33.4 µs  | 76.2 µs | **2.3× slower** |
    /// | 32 × 64  | 2.21 ms  | 608 µs  | **3.6× faster** |
    /// | 256 × 128 | 26.5 ms | 4.26 ms | **6.2× faster** |
    /// | 2048 × 256 | 808 ms | 59.7 ms | **13.5× faster** |
    ///
    /// A handful of short documents measurably *regresses* — the same shape of
    /// result the per-word audit predicted for naive word-level parallelism, at
    /// a smaller scale — but real batches (dozens-plus of paragraph-sized
    /// documents) win by 3.6–13.5× on this hardware.
    ///
    /// # When to reach for it
    ///
    /// Use this over a plain `docs.iter().map(|d|
    /// self.tokenize_and_stem(d, keep_stops)).collect()` loop when you have many
    /// (dozens or more) independent documents *and* each is at least
    /// paragraph-sized (tens of words) — the `32 × 64` row above is roughly where
    /// the win starts. Below that, the sequential loop is simpler and, per the
    /// table above, often faster: rayon still has to spin up its global thread
    /// pool on first use, and a handful of short documents will not amortise
    /// that. Do not reach for this to parallelize *within* one document — that is
    /// the per-word case the crate's benchmarks argue against.
    ///
    /// # Cost
    ///
    /// One `Vec<String>` allocation per document (same as calling
    /// [`Self::tokenize_and_stem`] that many times) plus the outer `Vec` that
    /// collects them; rayon's `par_iter().map().collect()` is order-preserving,
    /// so `out[i]` is always `docs[i]`'s result. `Self` must be `Sync`, because
    /// every task borrows it: this is why [`crate::PorterStemmerNl`], whose sticky
    /// `suffixeRemoved` flag lives in a `Cell`, does not get this method —
    /// running it from multiple threads at once would make that stemmer's output
    /// depend on scheduling order, and the type system refuses it instead of
    /// letting that happen silently. Construct one [`PorterStemmerNl`][crate::PorterStemmerNl]
    /// per document and fall back to a sequential loop there.
    ///
    /// This does not spawn or configure any thread pool; it uses whatever global
    /// rayon pool is already active (or the default one, created lazily on first
    /// use).
    #[cfg(feature = "parallel")]
    fn par_tokenize_and_stem_batch(&self, docs: &[&str], keep_stops: bool) -> Vec<Vec<String>>
    where
        Self: Sized + Sync,
    {
        use rayon::prelude::*;

        docs.par_iter()
            .map(|doc| self.tokenize_and_stem(doc, keep_stops))
            .collect()
    }
}

/// The lazy token-and-stem iterator returned by [`TokenizeAndStem::stems`].
#[derive(Debug)]
pub struct Stems<'a, S> {
    stemmer: &'a S,
    buf: Cow<'a, str>,
    pos: usize,
    keep_stops: bool,
}

impl<S: TokenizeAndStem> Iterator for Stems<'_, S> {
    type Item = String;

    fn next(&mut self) -> Option<String> {
        loop {
            let (start, end) = next_run(&self.buf, self.pos, S::is_word_char)?;
            self.pos = end;
            let raw = &self.buf[start..end];

            // Lowercase at most once, even when both axes ask for it.
            let lowered: Option<String> =
                if S::FILTER_ON == Casing::Lower || S::STEM_ON == Casing::Lower {
                    Some(raw.to_lowercase())
                } else {
                    None
                };
            let pick = |casing: Casing| -> &str {
                match casing {
                    Casing::Raw => raw,
                    Casing::Lower => lowered.as_deref().unwrap_or(raw),
                }
            };

            if !self.keep_stops && S::is_stop_word(pick(S::FILTER_ON)) {
                continue;
            }
            let input = pick(S::STEM_ON);
            return Some(if S::gate(input) {
                self.stemmer.stem_token(input)
            } else {
                input.to_owned()
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use verbora_tokenizers::{
        AggressiveTokenizer, AggressiveTokenizerDe, AggressiveTokenizerFr, AggressiveTokenizerId,
        AggressiveTokenizerRu, Tokenize, classes,
    };

    use super::*;

    /// Collects the runs `next_run` finds, so they can be compared with the
    /// tokenizers crate's own (already parity-verified) output.
    fn runs(text: &str, is_word: fn(char) -> bool) -> Vec<&str> {
        let mut out = Vec::new();
        let mut pos = 0;
        while let Some((a, b)) = next_run(text, pos, is_word) {
            out.push(&text[a..b]);
            pos = b;
        }
        out
    }

    #[test]
    fn cached_variant_matches_the_plain_pipeline() {
        use crate::{PorterStemmer, PorterStemmerEs, PorterStemmerRu};

        let texts = [
            "My dog is very fun to play with",
            "Los trabajadores nacionales trabajan rápidamente",
            "мама мыла раму важнейшими способами",
            "",
            "a-b 123 😀",
        ];
        let mut cache = HashMap::new();
        for text in texts {
            for keep_stops in [false, true] {
                let en = PorterStemmer::new();
                assert_eq!(
                    en.tokenize_and_stem_cached(text, keep_stops, &mut cache),
                    en.tokenize_and_stem(text, keep_stops),
                    "en: {text:?}"
                );
            }
        }
        // Run the same texts again so every lookup is a cache hit.
        for text in texts {
            let en = PorterStemmer::new();
            assert_eq!(
                en.tokenize_and_stem_cached(text, false, &mut cache),
                en.tokenize_and_stem(text, false),
                "en, warm cache: {text:?}"
            );
        }
        let mut cache = HashMap::new();
        let es = PorterStemmerEs::new();
        assert_eq!(
            es.tokenize_and_stem_cached(texts[1], false, &mut cache),
            es.tokenize_and_stem(texts[1], false)
        );
        let mut cache = HashMap::new();
        let ru = PorterStemmerRu::new();
        assert_eq!(
            ru.tokenize_and_stem_cached(texts[2], false, &mut cache),
            ru.tokenize_and_stem(texts[2], false)
        );
    }

    /// The cache is trusted: a pre-seeded entry is served verbatim. This pins
    /// the documented contract (the owner is responsible for the map's
    /// contents), which the classifier crate's memoization relies on.
    #[test]
    fn cache_entries_are_reused_not_recomputed() {
        use crate::PorterStemmer;

        let mut cache = HashMap::new();
        cache.insert("running".to_owned(), "SEEDED".to_owned());
        assert_eq!(
            PorterStemmer::new().tokenize_and_stem_cached("running dogs", false, &mut cache),
            ["SEEDED", "dog"]
        );
        // The miss was inserted for next time.
        assert_eq!(cache.get("dogs").map(String::as_str), Some("dog"));
    }

    /// `PorterStemmerNl` is the one impure stemmer: the cached entry point
    /// must ignore the cache — a poisoned entry changes nothing, and the
    /// sticky-flag state advances exactly as the plain pipeline would.
    #[test]
    fn nl_ignores_the_cache_because_its_stem_is_impure() {
        use crate::PorterStemmerNl;

        const {
            assert!(!<PorterStemmerNl as TokenizeAndStem>::PURE_STEM);
        }
        let mut cache = HashMap::new();
        cache.insert("onaantastbar".to_owned(), "POISON".to_owned());
        let nl = PorterStemmerNl::new();
        // Fresh stemmer: the flag is unset, so the word is untouched — and the
        // poisoned cache entry must NOT leak into the output.
        assert_eq!(
            nl.tokenize_and_stem_cached("onaantastbar", false, &mut cache),
            ["onaantastbar"]
        );
        // Trip the sticky flag, then observe the state-dependent stem — the
        // exact behaviour a cache hit would have destroyed.
        let _ = nl.tokenize_and_stem_cached("lichte", false, &mut cache);
        assert_eq!(
            nl.tokenize_and_stem_cached("onaantastbar", false, &mut cache),
            ["onaantast"]
        );
    }

    #[test]
    fn run_scanning_agrees_with_the_verified_tokenizers() {
        let samples = [
            "",
            "   ",
            "The quick brown fox",
            "it's a-b c/d",
            "Das Haus ist schön und die Häuser sind schöner.",
            "Le petit cheval de manège",
            "мама мыла раму",
            "buku-buku itu dibaca",
            "a😀b emoji",
            "tabs\tand\nnewlines",
            "!!!",
            "-leading and trailing-",
        ];
        for s in samples {
            assert_eq!(
                runs(s, classes::is_word_en),
                AggressiveTokenizer::new().tokenize(s),
                "en: {s:?}"
            );
            assert_eq!(
                runs(s, classes::is_word_de),
                AggressiveTokenizerDe::new().tokenize(s),
                "de: {s:?}"
            );
            assert_eq!(
                runs(s, classes::is_word_fr),
                AggressiveTokenizerFr::new().tokenize(s),
                "fr: {s:?}"
            );
            assert_eq!(
                runs(s, classes::is_word_ru),
                AggressiveTokenizerRu::new().tokenize(s),
                "ru: {s:?}"
            );
            let lower = s.to_lowercase();
            assert_eq!(
                runs(&lower, classes::is_word_id),
                AggressiveTokenizerId::new().tokenize(&lower),
                "id: {s:?}"
            );
        }
    }
}
