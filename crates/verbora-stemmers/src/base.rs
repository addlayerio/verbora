//! `tokenize_and_stem`, expressed once.
//!
//! The module is private and its whole contract lives on [`TokenizeAndStem`],
//! [`Stems`] and [`Casing`], which the crate root re-exports. Read the trait.

use std::borrow::Cow;
use std::collections::HashMap;
use std::hash::BuildHasher;

use verbora_tokenizers::{BorrowingTokenizer, WordTokenizer};

/// Which form of a token a step looks at.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Casing {
    /// The token exactly as the tokenizer produced it.
    Raw,
    /// The token lowercased.
    Lower,
}

/// The next UAX #29 word of `s` at or after byte offset `from`, as a byte
/// range, optionally extended across word-internal hyphens.
///
/// Restarting the boundary scan at `from` is sound because `from` is always
/// either `0` or the end of a previously yielded word, and both are UAX #29
/// break positions: no rule spans a break, and the one rule with unbounded
/// lookbehind (WB15/WB16's regional-indicator parity) restarts its count at
/// every break. `stems_walk_agrees_with_one_whole_tokenizer_pass` checks that
/// claim against a single full pass, over input carrying regional indicators,
/// ZWJ sequences and combining marks.
///
/// `join_hyphens` is [`TokenizeAndStem::HYPHEN_JOINS_LETTERS`]; see there for
/// the rule and why exactly one language sets it.
fn next_word(s: &str, from: usize, join_hyphens: bool) -> Option<(usize, usize)> {
    let rest = s.get(from..)?;
    let token = WordTokenizer.tokens(rest).next()?;
    // `token` is a slice of `rest`, so its address gives its offset.
    let offset = token.as_ptr() as usize - rest.as_ptr() as usize;
    let mut end = offset + token.len();
    if join_hyphens {
        while let Some(extended) = hyphen_continuation(rest, end) {
            end = extended;
        }
    }
    Some((from + offset, from + end))
}

/// If the word ending at `end` is continued through a hyphen, where the
/// continuation ends.
///
/// The hyphen must sit directly between two `Alphabetic` scalars — no spaces,
/// no digits on either side — so `anak-anak` is one word while `12-05-2020`,
/// `-leading` and `trailing-` keep the untailored UAX #29 boundaries.
fn hyphen_continuation(rest: &str, end: usize) -> Option<usize> {
    if !rest[end..].starts_with('-') {
        return None;
    }
    if !rest[..end]
        .chars()
        .next_back()
        .is_some_and(char::is_alphabetic)
    {
        return None;
    }
    let after = end + '-'.len_utf8();
    let tail = rest.get(after..)?;
    if !tail.chars().next().is_some_and(char::is_alphabetic) {
        return None;
    }
    let next = WordTokenizer.tokens(tail).next()?;
    // The continuation must begin at the hyphen, not somewhere past it.
    let start = next.as_ptr() as usize - tail.as_ptr() as usize;
    (start == 0).then(|| after + next.len())
}

/// Turning text into stems: tokenize, filter stop words, stem what is left.
///
/// Fifteen types in this crate implement this trait, and their recipes are
/// almost — but not quite — copies of one another. The differences are small
/// and load-bearing, so they are named as associated constants rather than
/// buried in fifteen near-identical loops:
///
/// | | [`prepare`] rewrites the text to | stop-word test reads | `stem` receives | gate |
/// |---|---|---|---|---|
/// | en, Lancaster, id | **its lowercase** | the token | the token | none |
/// | fa | itself minus one literal run | the token | the token | none |
/// | de, es, it, nl, ru, uk | itself | the **raw** token | the lowercased token | yes |
/// | fr, Carry | itself | the **lowercased** token | the lowercased token | yes |
/// | no, sv, pt | itself | the lowercased token | the **raw** token | none |
///
/// Fifteen rows collapse to five because the four axes are the only degrees of
/// freedom there are: which text is tokenized, which string is filtered on,
/// which string is stemmed, and whether a gate exempts the token from
/// stemming. [`Stems`] implements the five recipes once.
///
/// A single policy applied to all fifteen would change the token stream for
/// most of them, and silently. German keeps a capitalised `"Das"` because its
/// stop-word list is consulted with the **raw** token, while lowercase `"ist"`,
/// `"und"` and `"die"` are dropped.
///
/// Two rows carry a caveat the table cannot hold. `en`, `Lancaster` and `id`
/// lowercase the *document*, not each token, because `to_lowercase` can change
/// a string's length and therefore move token boundaries. And Norwegian and
/// Swedish do **not** fold diacritics here, because `å ä ö` and `æ ø å` are
/// letters of those alphabets rather than accents — see
/// [`crate::PorterStemmerSv`] and [`crate::PorterStemmerNo`] for the decision
/// and the 124 stop words the fold had made unreachable.
///
/// [`prepare`]: TokenizeAndStem::prepare
///
/// # Tokenization
///
/// Every language uses [`verbora_tokenizers::WordTokenizer`] — the UAX #29 word
/// segments containing a letter or a digit. There is no per-language character
/// class: a repertoire of accented Latin letters and Cyrillic ranges is a
/// character repertoire rather than a linguistic rule, and `Word_Break` covers
/// the whole repertoire at once. The visible consequences are that
/// `"well-known"` and `"and/or"` stem as two tokens each while `"don't"`,
/// `"3.14"`, `"1,000"` and `"node_js"` stem as one, and that accented and
/// non-Latin text is not cut at every character some per-language class
/// happened to omit.
///
/// The one tailoring is [`Self::HYPHEN_JOINS_LETTERS`], which Indonesian sets.
/// UAX #29 §4 presents its rules as a *default* that languages are expected to
/// tailor, and Indonesian marks reduplication with `U+002D` inside what its
/// orthography treats as one word — `buku-buku`, `malaikat-malaikat-nya`.
/// Leaving that at the default breaks a single lexeme into pieces none of
/// which is the lexeme.
///
/// # Choosing the right entry point
///
/// Four methods turn text into stems. They compute the same token stream and
/// differ only in what they allocate and when.
///
/// | | Returns | Allocates | Use when |
/// |---|---|---|---|
/// | [`stems`](Self::stems) | a lazy iterator | one `String` per stem, on demand | you want the first few stems of a large document, or you are feeding another iterator |
/// | [`tokenize_and_stem`](Self::tokenize_and_stem) | `Vec<String>` | the above plus one `Vec` | **the default.** You want all the stems and you want them in a collection |
/// | [`tokenize_and_stem_cached`](Self::tokenize_and_stem_cached) | `Vec<String>` | the above, minus one `stem` call per repeated token | you process many documents over a small vocabulary and own a cache to hand in |
/// | [`par_tokenize_and_stem_batch`](Self::par_tokenize_and_stem_batch) | `Vec<Vec<String>>` | one `Vec` per document plus the outer one | you have dozens or more independent documents, each at least paragraph-sized (`parallel` feature) |
///
/// [`stems`](Self::stems) is the primitive and the other three are built from
/// it; `tokenize_and_stem` is literally `stems(..).collect()`. Start there. The
/// cached form is worth its extra argument only when tokens actually repeat —
/// it is the same walk with one `HashMap` probe substituted for one `stem`
/// call, so on all-distinct tokens it is strictly slower. The parallel form
/// measurably *regresses* a handful of short documents; see its own
/// documentation for the crossover.
///
/// A caller who has already tokenized should skip all four and call the
/// stemmer's own inherent `stem` per token.
///
/// # Laziness
///
/// [`Stems`] holds the prepared text (borrowed when no rewrite was needed) and
/// one byte cursor, so a caller that only wants the first few stems of a large
/// document pays for only those.
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

    /// Whether `U+002D` between two letters continues a word instead of ending
    /// it — a UAX #29 §4 tailoring, off by default.
    ///
    /// When `true`, a hyphen that is immediately preceded *and* immediately
    /// followed by an `Alphabetic` scalar does not break a word, so
    /// `"anak-anak"` is one token. Everything else keeps the untailored
    /// boundaries: `"12-05-2020"` is three tokens (the sides are digits),
    /// `"-leading"`, `"trailing-"` and `"a - b"` are unaffected, and a run of
    /// hyphens is never a word at all.
    ///
    /// # Why this exists, and why only Indonesian sets it
    ///
    /// UAX #29 presents its word rules as a *default* that languages are
    /// expected to tailor, and Indonesian orthography writes reduplication —
    /// its plural — as one word joined by a hyphen: `buku-buku` ("books"),
    /// `malaikat-malaikat-nya` ("his angels"). The crate's own data agrees:
    /// 335 of the 29,932 Indonesian roots and 22 of the 809 Indonesian stop
    /// words are single lexemes spelled with a hyphen, and
    /// [`crate::StemmerId`]'s entire plural branch takes a hyphenated token as
    /// its input. Under the untailored default none of that is reachable
    /// through this pipeline: the lexeme arrives as two tokens, neither of
    /// which is in the dictionary or on the list.
    ///
    /// No other language in this crate carries a hyphenated entry in either
    /// its dictionary or its stop-word list, and in English, German, French
    /// and the rest a hyphen joins two words that each mean something on their
    /// own — `well-known` really is `well` and `known`. Turning this on for
    /// them would merge tokens that ought to stay apart, so it stays off.
    const HYPHEN_JOINS_LETTERS: bool = false;

    /// Rewrites the text before tokenizing.
    ///
    /// English, Lancaster and Indonesian lowercase the whole document here,
    /// which is *not* the same as lowercasing each token afterwards:
    /// `to_lowercase` can change a string's length (`'İ'` becomes `i` +
    /// U+0307), so it can move token boundaries. Persian replaces one literal
    /// run with a space. Everything else — Norwegian and Swedish included —
    /// borrows unchanged; see [`crate::PorterStemmerSv`] and
    /// [`crate::PorterStemmerNo`] for why folding their diacritics here was
    /// wrong.
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
        while let Some((start, end)) = next_word(&buf, pos, Self::HYPHEN_JOINS_LETTERS) {
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
    /// `suffix_e_removed` flag lives in a `Cell`, does not get this method —
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
            let (start, end) = next_word(&self.buf, self.pos, S::HYPHEN_JOINS_LETTERS)?;
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
    use super::*;

    /// Collects the words `next_word` finds by walking, so they can be compared
    /// with a single whole-input pass of the same tokenizer.
    fn walk(text: &str) -> Vec<&str> {
        walk_with(text, false)
    }

    fn walk_with(text: &str, join_hyphens: bool) -> Vec<&str> {
        let mut out = Vec::new();
        let mut pos = 0;
        while let Some((a, b)) = next_word(text, pos, join_hyphens) {
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
        // Indonesian is the one stemmer whose walk is tailored, so the cached
        // walk has to carry the tailoring too.
        let mut cache = HashMap::new();
        let id = crate::StemmerId::new();
        for text in ["buku-buku itu dibaca", "malaikat-malaikat-nya", "a-b 12-05"] {
            for keep_stops in [false, true] {
                assert_eq!(
                    id.tokenize_and_stem_cached(text, keep_stops, &mut cache),
                    id.tokenize_and_stem(text, keep_stops),
                    "id: {text:?}"
                );
            }
        }
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

    /// The lazy walk must agree, token for token, with a single whole-input
    /// pass of the same tokenizer.
    ///
    /// `next_word` restarts the UAX #29 boundary scan at the end of the
    /// previous word rather than carrying one iterator across the whole
    /// document, which is only sound because every restart point is a break
    /// position. The samples carry the constructs where that could fail:
    /// regional-indicator pairs (WB15/WB16 count parity backwards without
    /// bound), ZWJ emoji sequences (WB3c), combining marks attaching under WB4,
    /// and `CR LF` (WB3). A whitespace-and-ASCII sample set reaches none of
    /// them.
    #[test]
    fn stems_walk_agrees_with_one_whole_tokenizer_pass() {
        let samples = [
            "",
            "   ",
            "The quick brown fox",
            "it's a-b c/d 3.14 1,000 node_js a:b",
            "Das Haus ist schön und die Häuser sind schöner.",
            "Le petit cheval de manège",
            "мама мыла раму",
            "buku-buku itu dibaca",
            "a😀b emoji",
            "tabs\tand\nnewlines",
            "a\r\nb",
            "!!!",
            "-leading and trailing-",
            "\u{301}leading combining mark",
            "trailing combining mark\u{301}",
            "🇦🇧🇨 flags 🇦 lone",
            "a🇦🇧🇨🇩b",
            "👨\u{200d}👩\u{200d}👧\u{200d}👦 family",
            "日本語 と ภาษาไทย と العربية",
            "١٢٣ ½ \u{2160}",
        ];
        for s in samples {
            assert_eq!(
                walk(s),
                WordTokenizer.tokenize_borrowed(s),
                "walk disagrees with one pass on {s:?}"
            );
        }
    }

    /// The moved boundaries, at the level a stemmer consumer sees them: the
    /// token stream `Stems` walks is UAX #29's, so hyphens and slashes split
    /// and apostrophes, decimal points, thousands separators, colons and
    /// underscores do not.
    #[test]
    fn the_stemmer_token_stream_is_uax29() {
        use crate::PorterStemmer;

        let en = PorterStemmer::new();
        // `keep_stops = true` so the stop-word list cannot hide a boundary.
        assert_eq!(
            en.tokenize_and_stem("well-known and/or don't 3.14 1,000 node_js a:b", true),
            [
                "well", "known", "and", "or", "don't", "3.14", "1,000", "node_j", "a:b"
            ]
        );
        // Non-ASCII letters are letters, not separators.
        assert_eq!(en.tokenize_and_stem("café naïve", true), ["café", "naïv"]);
    }

    /// Six languages consult the stop-word list with the **raw** token, so a
    /// tokenizer that folded case would start dropping capitalised stop words
    /// with no compile error. The tokenizer does not fold, and this is the
    /// tripwire that fires if that ever changes.
    #[test]
    fn the_tokenizer_does_not_fold_case() {
        use crate::PorterStemmerDe;

        let de = PorterStemmerDe::new();
        // German is FILTER_ON = Raw, STEM_ON = Lower: the stop-word list is
        // consulted with the token exactly as the tokenizer produced it, and
        // the emitted value is the lowercased stem. "das" and "ist" are stop
        // words; capitalised "Das" is a different string, so it survives the
        // filter and is then emitted folded.
        assert_eq!(
            de.tokenize_and_stem("Das ist das Haus", false),
            ["das", "haus"]
        );
        // Lowercase "das" in the same position *is* dropped — which is only
        // observable because the tokenizer handed the filter the raw token.
        assert_eq!(de.tokenize_and_stem("das ist das Haus", false), ["haus"]);
    }

    /// `HYPHEN_JOINS_LETTERS` is a tailoring of exactly one rule, and the
    /// boundary cases are where a tailoring turns into a second tokenizer.
    /// Both sides of the hyphen must be `Alphabetic` and the hyphen must be
    /// adjacent to both.
    #[test]
    fn hyphen_joining_is_narrow_and_off_by_default() {
        // Letters on both sides, ASCII and not, and any number of hyphens.
        assert_eq!(walk_with("buku-buku", true), ["buku-buku"]);
        assert_eq!(
            walk_with("malaikat-malaikat-nya", true),
            ["malaikat-malaikat-nya"]
        );
        assert_eq!(walk_with("blå-bær", true), ["blå-bær"]);
        // Digits are not letters, so a date keeps the default boundaries.
        assert_eq!(walk_with("12-05-2020", true), ["12", "05", "2020"]);
        assert_eq!(walk_with("a-1", true), ["a", "1"]);
        assert_eq!(walk_with("1-a", true), ["1", "a"]);
        // The hyphen must touch both words.
        assert_eq!(walk_with("a - b", true), ["a", "b"]);
        assert_eq!(walk_with("a- b", true), ["a", "b"]);
        assert_eq!(walk_with("a -b", true), ["a", "b"]);
        assert_eq!(walk_with("a--b", true), ["a", "b"]);
        // Nothing on one side at all.
        assert_eq!(walk_with("-leading", true), ["leading"]);
        assert_eq!(walk_with("trailing-", true), ["trailing"]);
        assert_eq!(walk_with("---", true), Vec::<&str>::new());
        // Off, the same inputs are exactly UAX #29's.
        for s in [
            "buku-buku",
            "malaikat-malaikat-nya",
            "blå-bær",
            "12-05-2020",
            "a--b",
            "-leading",
            "trailing-",
            "---",
        ] {
            assert_eq!(
                walk_with(s, false),
                WordTokenizer.tokenize_borrowed(s),
                "the untailored walk moved on {s:?}"
            );
        }
    }

    /// Whatever the tailoring, tokens stay contiguous, non-overlapping slices
    /// of the input in increasing order — the invariant every offset consumer
    /// depends on, and the one a hand-rolled extension is most likely to break.
    #[test]
    fn joined_tokens_are_still_ordered_slices_of_the_input() {
        for s in [
            "buku-buku itu dibaca",
            "a-b-c-d",
            "kira-kira 12-05-2020 well-known",
            "-a-",
            "é-è",
            "a-\u{301}b",
            "🇦🇧-🇨",
            "",
        ] {
            let mut end = 0;
            let mut pos = 0;
            while let Some((a, b)) = next_word(s, pos, true) {
                assert!(a >= end, "{s:?}: token at {a} overlaps the previous end");
                assert!(a < b && b <= s.len(), "{s:?}: bad range {a}..{b}");
                assert!(s.is_char_boundary(a) && s.is_char_boundary(b));
                end = b;
                pos = b;
            }
        }
    }

    /// Only Indonesian tailors the hyphen; the default must stay off for
    /// everything else, or `well-known` silently becomes one English token.
    #[test]
    fn only_indonesian_joins_hyphens() {
        use crate::{
            CarryStemmerFr, LancasterStemmer, PorterStemmer, PorterStemmerDe, PorterStemmerEs,
            PorterStemmerFa, PorterStemmerFr, PorterStemmerIt, PorterStemmerNl, PorterStemmerNo,
            PorterStemmerPt, PorterStemmerRu, PorterStemmerSv, PorterStemmerUk, StemmerId,
        };

        const fn joins<S: TokenizeAndStem>() -> bool {
            S::HYPHEN_JOINS_LETTERS
        }
        const {
            assert!(joins::<StemmerId>());
            assert!(!joins::<PorterStemmer>());
            assert!(!joins::<LancasterStemmer>());
            assert!(!joins::<CarryStemmerFr>());
            assert!(!joins::<PorterStemmerDe>());
            assert!(!joins::<PorterStemmerEs>());
            assert!(!joins::<PorterStemmerFa>());
            assert!(!joins::<PorterStemmerFr>());
            assert!(!joins::<PorterStemmerIt>());
            assert!(!joins::<PorterStemmerNl>());
            assert!(!joins::<PorterStemmerNo>());
            assert!(!joins::<PorterStemmerPt>());
            assert!(!joins::<PorterStemmerRu>());
            assert!(!joins::<PorterStemmerSv>());
            assert!(!joins::<PorterStemmerUk>());
        }
        // The same text, one tailoring apart.
        assert_eq!(
            PorterStemmer::new().tokenize_and_stem("well-known", true),
            ["well", "known"]
        );
        assert_eq!(
            StemmerId::new().tokenize_and_stem("buku-buku", true),
            ["buku"]
        );
    }
}
