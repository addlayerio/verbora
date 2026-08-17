//! Chunked batch phonetic encoding, parallelised with [`rayon`].
//!
//! Gated behind the `parallel` feature. Everything else in this crate works
//! identically with or without it — this module adds *one* way to run the
//! existing sequential encoders across many independent words at once; it does
//! not change how a single word is encoded.
//!
//! # Why this exists
//!
//! [`Phonetic::process`](verbora_core::Phonetic::process) and
//! [`DoubleKeyPhonetic::process_double`](verbora_core::DoubleKeyPhonetic::process_double)
//! are cheap: this crate's own benchmarks put a single call at roughly 40–200 ns
//! depending on the encoder and the word (see `benches/phonetics.rs`). A
//! [`rayon`] task costs on the order of tens to low hundreds of nanoseconds to
//! schedule, so naively dispatching one task per word —
//! `words.par_iter().map(soundex)` — puts scheduling overhead in the same
//! order of magnitude as the work itself.
//!
//! Measured behaviour of that naive form (`benches/phonetics.rs`'s
//! `parallel_batch` group — run it with
//! `cargo bench -p verbora-phonetics --features parallel -- parallel_batch`)
//! is **noisy** rather than uniformly bad: across repeated runs on a 32-core
//! development machine it ranged from more than 2× *slower* than the plain
//! sequential loop to several times *faster*, depending on host load and
//! Rayon's internal work-stealing state at the time — the exact multipliers
//! are not portable to your machine, but the run-to-run swing itself is the
//! case against relying on it: you cannot predict which side of the coin you
//! get. [`par_encode_batch`] and [`par_encode_double_batch`] hand each Rayon
//! task an explicit **chunk** of words instead of one word; across every
//! chunk size and word count this crate measured (64–4096 words per chunk,
//! 500–100,000 words total), chunking never regressed as badly as the naive
//! form's worst runs did, and a small chunk size ([`DEFAULT_CHUNK_SIZE`]) was
//! consistently at or near the fastest — predictability, as much as raw
//! speed, is the point.
//!
//! # When to reach for this vs the sequential loop
//!
//! - A handful of words, or words arriving one at a time (e.g. per HTTP
//!   request): call [`Phonetic::process`](verbora_core::Phonetic::process)
//!   directly, or loop with `.iter().map(...)`. Do not reach for this module.
//! - Building an index over tens of thousands of words or more, where the
//!   encoding only has to happen once: this is the intended use, and where
//!   the measurements above were taken.
//! - Anywhere in between: measure. The gain was visible even at a few
//!   hundred words in this crate's own measurements, but that was on a
//!   32-core machine with an idle thread pool; on a smaller machine, or one
//!   already busy with other work, the crossover point moves and only a
//!   measurement on your own hardware and workload will tell you where.
//!
//! # What it costs
//!
//! - Each function allocates one output `Vec` sized to `tokens.len()`, exactly
//!   like the sequential `.iter().map(...).collect()` it wraps — no additional
//!   buffering per chunk. `par_chunks` and `flat_map_iter` do not allocate
//!   intermediate collections; each Rayon task streams its `process` calls
//!   directly into the shared output via `collect`'s parallel consumer.
//! - `chunk_size` is a required argument, not a hidden default, in keeping
//!   with this project's rule that Rayon-backed APIs are opted into explicitly
//!   by the caller rather than silently changing behaviour based on which
//!   Cargo features happen to be enabled. [`DEFAULT_CHUNK_SIZE`] documents a
//!   measured starting point; tune from there for your own word lengths and
//!   core count.
//! - `phonetic: &P` is shared read-only across every worker thread. All four
//!   encoders in this crate ([`SoundEx`](crate::SoundEx), [`Metaphone`](crate::Metaphone),
//!   [`SoundExDM`](crate::SoundExDM), [`DoubleMetaphone`](crate::DoubleMetaphone))
//!   are zero-sized `Copy` types with no interior mutability, so sharing one
//!   `&P` across threads is free and requires no locking.
//! - Output order matches input order. Rayon's `map`/`collect` never
//!   reorders — this is not a property this module has to implement, it falls
//!   out of using `par_chunks().flat_map_iter().collect()` instead of an
//!   unordered reduction.
//! - This module calls the exact same [`Phonetic::process`](verbora_core::Phonetic::process)
//!   / [`DoubleKeyPhonetic::process_double`](verbora_core::DoubleKeyPhonetic::process_double)
//!   that the sequential API uses. There is no second implementation of
//!   Soundex, Metaphone, Double Metaphone or Daitch–Mokotoff here to keep in
//!   sync with the sequential one — only a chunked dispatch strategy around
//!   it. If the sequential encoder's output ever changes, this module's output
//!   changes with it automatically.

use rayon::prelude::*;

use verbora_core::{DoubleKeyPhonetic, Phonetic};

/// A reasonable starting chunk size for [`par_encode_batch`] and
/// [`par_encode_double_batch`] on typical English word lists (a handful of
/// characters to a few dozen), tuned against `SoundEx` on a 32-core machine.
///
/// This is a starting point for tuning, not a claim that it is optimal for
/// your data: word length, alphabet (ASCII vs code paths that promote to
/// UTF-16), core count and which encoder you use all shift the ideal value.
/// `benches/phonetics.rs`'s `parallel_batch` group measures 64, 256, 1024 and
/// 4096 against the naive per-word case and the sequential baseline; run it
/// with `cargo bench -p verbora-phonetics --features parallel -- parallel_batch`
/// to see the numbers this default was picked from, on your own hardware.
///
/// The measured pattern that motivates `64` specifically: larger chunk sizes
/// (1024, 4096) *sometimes* matched or beat it, but only when the input was
/// large enough that even a large chunk size still produced many more chunks
/// than there are cores; at the smaller end of this API's intended range
/// (tens of thousands of words) a large chunk size can produce *fewer chunks
/// than cores*, which idles most of the thread pool. `64` produced enough
/// chunks to keep every core fed across every size measured (500 to 100,000
/// words) while keeping each chunk's real work — tens of encoder calls —
/// comfortably above Rayon's per-task overhead. It was consistently at or
/// near the fastest option across repeated runs, not necessarily the single
/// fastest in every one of them.
pub const DEFAULT_CHUNK_SIZE: usize = 64;

/// Encodes `tokens` in parallel, in chunks, using `phonetic`.
///
/// Equivalent to `tokens.iter().map(|t| phonetic.process(t)).collect()` —
/// same encoder, same per-word behaviour, same output order — split across
/// Rayon's thread pool in groups of `chunk_size` words per task instead of one
/// task per word. See the [module docs](self) for why the chunking matters.
///
/// `chunk_size` is clamped to at least 1 (an input of `0` would otherwise
/// panic inside `par_chunks`); passing `1` is legal and reproduces the naive
/// one-task-per-word dispatch this module's docs recommend against — useful
/// for the parity tests and the benchmark that measures the difference, not
/// for production use. [`DEFAULT_CHUNK_SIZE`] is a reasonable value to start
/// tuning from.
///
/// # Examples
///
/// ```
/// use verbora_phonetics::{SoundEx, par_encode_batch};
///
/// let soundex = SoundEx::new();
/// let words = ["Robert", "Rupert", "phonetics"];
/// let keys = par_encode_batch(&soundex, &words, 2);
/// assert_eq!(keys, ["R163", "R163", "P532"]);
/// ```
pub fn par_encode_batch<P>(phonetic: &P, tokens: &[&str], chunk_size: usize) -> Vec<String>
where
    P: Phonetic + Sync,
{
    let chunk_size = chunk_size.max(1);
    tokens
        .par_chunks(chunk_size)
        .flat_map_iter(|chunk| chunk.iter().map(|token| phonetic.process(token)))
        .collect()
}

/// As [`par_encode_batch`], for the two-key encoders
/// ([`DoubleMetaphone`](crate::DoubleMetaphone)) that implement
/// [`DoubleKeyPhonetic`] rather than [`Phonetic`].
///
/// Equivalent to `tokens.iter().map(|t| phonetic.process_double(t)).collect()`,
/// chunked the same way as [`par_encode_batch`].
///
/// # Examples
///
/// ```
/// use verbora_phonetics::{DoubleMetaphone, par_encode_double_batch};
///
/// let dm = DoubleMetaphone::new();
/// let words = ["astromech", "Matrix"];
/// let keys = par_encode_double_batch(&dm, &words, 2);
/// assert_eq!(
///     keys,
///     [
///         ("ATRMX".to_owned(), "ATRMK".to_owned()),
///         ("MTRKS".to_owned(), "MTRKS".to_owned()),
///     ]
/// );
/// ```
pub fn par_encode_double_batch<P>(
    phonetic: &P,
    tokens: &[&str],
    chunk_size: usize,
) -> Vec<(String, String)>
where
    P: DoubleKeyPhonetic + Sync,
{
    let chunk_size = chunk_size.max(1);
    tokens
        .par_chunks(chunk_size)
        .flat_map_iter(|chunk| chunk.iter().map(|token| phonetic.process_double(token)))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{DoubleMetaphone, Metaphone, SoundEx, SoundExDM};

    /// Pathological inputs already known to the sequential test suites in
    /// `soundex.rs`, `metaphone.rs`, `double_metaphone.rs` and `dm_soundex.rs`:
    /// empty input, digits, the Daitch–Mokotoff `"undefined"` leak, an astral
    /// character that orphans a surrogate, accented and non-Latin letters,
    /// punctuation/whitespace, and a long run. Reused here rather than
    /// reinvented, per this crate's own test conventions.
    fn pathological_words() -> Vec<String> {
        let mut words: Vec<String> = [
            "",
            "a",
            "Ashcraft",
            "chemical",
            "12345",
            "B0",
            "😀",
            "ÉCOLE",
            "café",
            "naïve",
            "ß",
            "ﬁ",
            "Москва",
            "日本語",
            "a-b",
            "it's",
            "/path",
            "  spaces  ",
            "(abc",
            "supercalifragilisticexpialidocious",
        ]
        .into_iter()
        .map(str::to_owned)
        .collect();
        words.push("a".repeat(500));
        words
    }

    /// Asserts `par_encode_batch` matches the sequential loop over `tokens`,
    /// for several chunk sizes including `1` (the naive, discouraged case) and
    /// `0` (which must clamp rather than panic).
    fn assert_batch_matches_sequential<P: Phonetic + Sync>(phonetic: &P, tokens: &[&str]) {
        let sequential: Vec<String> = tokens.iter().map(|t| phonetic.process(t)).collect();
        for chunk_size in [0, 1, 3, 7, 64, 1024, 100_000] {
            let parallel = par_encode_batch(phonetic, tokens, chunk_size);
            assert_eq!(
                sequential,
                parallel,
                "chunk_size={chunk_size} tokens.len()={}",
                tokens.len()
            );
        }
    }

    fn assert_double_batch_matches_sequential<P: DoubleKeyPhonetic + Sync>(
        phonetic: &P,
        tokens: &[&str],
    ) {
        let sequential: Vec<(String, String)> =
            tokens.iter().map(|t| phonetic.process_double(t)).collect();
        for chunk_size in [0, 1, 3, 7, 64, 1024, 100_000] {
            let parallel = par_encode_double_batch(phonetic, tokens, chunk_size);
            assert_eq!(
                sequential,
                parallel,
                "chunk_size={chunk_size} tokens.len()={}",
                tokens.len()
            );
        }
    }

    #[test]
    fn par_encode_batch_empty_input() {
        let soundex = SoundEx::new();
        assert_batch_matches_sequential(&soundex, &[]);
    }

    #[test]
    fn par_encode_batch_one_item() {
        let soundex = SoundEx::new();
        assert_batch_matches_sequential(&soundex, &["Robert"]);
    }

    #[test]
    fn par_encode_batch_pathological_and_unicode_soundex() {
        let soundex = SoundEx::new();
        let words = pathological_words();
        let tokens: Vec<&str> = words.iter().map(String::as_str).collect();
        assert_batch_matches_sequential(&soundex, &tokens);
    }

    #[test]
    fn par_encode_batch_pathological_and_unicode_metaphone() {
        let metaphone = Metaphone::new();
        let words = pathological_words();
        let tokens: Vec<&str> = words.iter().map(String::as_str).collect();
        assert_batch_matches_sequential(&metaphone, &tokens);
    }

    #[test]
    fn par_encode_batch_pathological_and_unicode_dm_soundex() {
        let dm = SoundExDM::new();
        let words = pathological_words();
        let tokens: Vec<&str> = words.iter().map(String::as_str).collect();
        assert_batch_matches_sequential(&dm, &tokens);
    }

    #[test]
    fn par_encode_double_batch_pathological_and_unicode() {
        let double = DoubleMetaphone::new();
        let words = pathological_words();
        let tokens: Vec<&str> = words.iter().map(String::as_str).collect();
        assert_double_batch_matches_sequential(&double, &tokens);
    }

    #[test]
    fn par_encode_double_batch_empty_and_one_item() {
        let double = DoubleMetaphone::new();
        assert_double_batch_matches_sequential(&double, &[]);
        assert_double_batch_matches_sequential(&double, &["astromech"]);
    }

    /// Many items, spanning many chunk boundaries at every chunk size under
    /// test (including sizes that do not evenly divide the input length), and
    /// large enough that a reordering bug would show up as a mismatch rather
    /// than passing by accident.
    #[test]
    fn par_encode_batch_many_items_preserves_order() {
        let soundex = SoundEx::new();
        let words = pathological_words();
        let mut tokens: Vec<&str> = Vec::new();
        for _ in 0..500 {
            tokens.extend(words.iter().map(String::as_str));
        }
        assert_eq!(tokens.len(), words.len() * 500);
        assert_batch_matches_sequential(&soundex, &tokens);
    }

    #[test]
    fn par_encode_double_batch_many_items_preserves_order() {
        let double = DoubleMetaphone::new();
        let words = pathological_words();
        let mut tokens: Vec<&str> = Vec::new();
        for _ in 0..500 {
            tokens.extend(words.iter().map(String::as_str));
        }
        assert_eq!(tokens.len(), words.len() * 500);
        assert_double_batch_matches_sequential(&double, &tokens);
    }
}
