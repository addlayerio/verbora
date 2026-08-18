//! The text-ingestion fast path: SWAR tokenizing plus dense per-document
//! counting.
//!
//! # Why this module exists
//!
//! `add_document` with a string input used to spend ~65% of its time running
//! the stop-word membership test once per token (a binary search over ~170
//! static strings) and most of the remainder hashing every token into the
//! interner and the per-document count map. Almost all of that work is
//! redundant: a token's stop-word status, its id, and its `observe`
//! classification are identical for every one of its occurrences. This module
//! runs the same logical pipeline with the per-token cost stripped to what
//! actually varies per token:
//!
//! * **Tokenizing** is two passes over the bytes. Pass one builds a bitmap of
//!   word characters (`[a-z0-9_]`) eight bytes at a time with SWAR arithmetic
//!   — safe `u64::from_le_bytes` loads and lane-parallel adds, no intrinsics
//!   — while simultaneously testing the *exact* predicate `lowercase_units`
//!   uses (any byte ≥ 0x80, or ASCII uppercase). Pass two walks maximal
//!   one-runs with `trailing_zeros`, yielding byte ranges identical to
//!   `WordTokenizer`'s tokens on this input class (proved exhaustively in the
//!   tests below).
//! * **Interning** resolves a short token (≤ 8 bytes) with one packed-key
//!   probe that also returns the term's intrinsic flags — see [`Interner`]'s
//!   representation notes.
//! * **Stop-word membership** is tested once per distinct term *per document*
//!   and cached in the per-document slot table. Per document — not per corpus
//!   — because the stop-word list is process-global and mutable
//!   ([`globals::is_stopword`]): re-resolving each document keeps every
//!   `add_document` call exactly as fresh as the per-token loop it replaces,
//!   with no cross-crate invalidation protocol to get wrong. Within one call
//!   the answer is treated as stable, the same latitude the per-token loop
//!   already has (a mid-call mutation of the global list races either way).
//! * **Counting** replaces the per-token hash-map probe with one load from a
//!   dense slot table indexed by term id, replicating
//!   [`BuiltDocument::observe`]'s reduce body — including the `__proto__`
//!   drop, the `__key` corruption, and issue #119's zero-before-count rule —
//!   branch for branch.
//!
//! # Parity
//!
//! The output is byte-exact `BuiltDocument`/[`Interner`] state: same term-id
//! assignment order, same entry insertion order, same `f64` counts, same
//! key mutations. It is verified by the randomized differential tests in
//! `tfidf.rs` (adversarial corpora against the reference per-token loop) and
//! by this module's tokenizer-equivalence tests. Input the bitmap cannot
//! certify (still non-ASCII after lowercasing) falls back to the real
//! `WordTokenizer` for scanning and keeps the fast interner/counter, so the
//! fallback differs only in speed, never in an observable result. A custom
//! tokenizer never reaches this module at all: `build_document` dispatches
//! here only while the process-global tokenizer is the default one.

use verbora_tokenizers::WordTokenizer;

use crate::document::{
    BuiltDocument, DocKey, Interner, TERM_KEY_PROPERTY, TERM_PROTO_METHOD, TERM_PROTO_PROPERTY,
    TermId,
};
use crate::globals;

// ---------------------------------------------------------------------------
// Scratch state
// ---------------------------------------------------------------------------

/// Reusable buffers for [`build_text`], owned by `TfIdf` so a corpus pays
/// their allocation once rather than once per document.
///
/// Invariant between calls: `slots` is all zeroes and `touched` is empty.
/// [`build_text`] restores both before returning by walking only the ids it
/// actually touched, so the reset costs O(distinct terms in the document),
/// never O(corpus vocabulary).
#[derive(Debug, Clone, Default)]
pub(crate) struct FastScratch {
    /// Word-character bitmap of the current document, one bit per byte.
    bitmap: Vec<u64>,
    /// Per-term-id state for the current document: `0` = not seen yet, else
    /// `SLOT_TOUCHED | stop-word bit | (entry index + 1)`.
    slots: Vec<u32>,
    /// Ids whose slot is nonzero, for the O(touched) reset.
    touched: Vec<TermId>,
}

/// Slot bit: this id has been seen in the current document. Needed so a
/// touched term with no entry and no stop-word bit (an unfiltered `__key` or
/// `__proto__`) still reads as nonzero.
const SLOT_TOUCHED: u32 = 1 << 31;
/// Slot bit: the term tested as a stop word for the current document.
const SLOT_STOP: u32 = 1 << 30;
/// Slot mask: entry index + 1, or zero when the term has no entry.
const SLOT_INDEX: u32 = SLOT_STOP - 1;

// ---------------------------------------------------------------------------
// SWAR word-character bitmap
// ---------------------------------------------------------------------------

/// Every lane's low bit set.
const LANES_LOW: u64 = 0x0101_0101_0101_0101;
/// Every lane's high bit set.
const LANES_HIGH: u64 = 0x8080_8080_8080_8080;

/// Per-byte `b >= n`, exact for bytes ≤ 0x7F: 0x80 marks true lanes.
///
/// `b + (0x80 - n) ≤ 0x7F + 0x80 = 0xFF` produces no carry between lanes, so
/// each lane's high bit is set iff that byte reaches `n`. A byte ≥ 0x80 can
/// carry into its neighbour and corrupt it — but any chunk containing one
/// also sets [`swar_bail`]'s non-ASCII mask, which discards the whole bitmap,
/// so the corruption is never read.
#[inline(always)]
fn swar_ge(x: u64, n: u8) -> u64 {
    x.wrapping_add(LANES_LOW * (0x80 - u64::from(n))) & LANES_HIGH
}

/// Word bytes (`[0-9a-z_]`) of a chunk: 0x80 marks word lanes.
///
/// Uppercase is deliberately absent from the class: a chunk containing an
/// uppercase byte bails (below), because `lowercase_units` would have
/// rewritten it before the tokenizer ever saw it.
#[inline(always)]
fn swar_word(x: u64) -> u64 {
    let digit = swar_ge(x, b'0') & !swar_ge(x, b'9' + 1);
    let lower = swar_ge(x, b'a') & !swar_ge(x, b'z' + 1);
    let under = swar_ge(x, b'_') & !swar_ge(x, b'_' + 1);
    digit | lower | under
}

/// Bail bytes: non-ASCII or ASCII uppercase — 0x80 marks offending lanes.
///
/// This is byte-for-byte the predicate `lowercase_units` scans for, so
/// "bitmap certified" and "lowercasing is the identity" are the same fact.
#[inline(always)]
fn swar_bail(x: u64) -> u64 {
    let non_ascii = x & LANES_HIGH;
    let upper = swar_ge(x, b'A') & !swar_ge(x, b'Z' + 1) & !non_ascii;
    non_ascii | upper
}

/// Packs a 0x80-per-lane mask into 8 bits (bit *i* = byte *i*).
#[inline(always)]
fn movemask(m: u64) -> u64 {
    (m >> 7).wrapping_mul(0x0102_0408_1020_4080) >> 56
}

/// Pass 1: fills `bm` with the word-character bitmap of `bytes`, one bit per
/// byte. Returns `false` when a non-ASCII or uppercase byte exists — the
/// caller must lowercase and retry, or take the char-exact path.
///
/// This single pass replaces two scans of the old path: `lowercase_units`'
/// may-change probe and the tokenizer's char-class walk.
fn build_bitmap(bytes: &[u8], bm: &mut Vec<u64>) -> bool {
    bm.clear();
    bm.reserve(bytes.len().div_ceil(64));
    let mut chunks = bytes.chunks_exact(8);
    let mut acc: u64 = 0;
    let mut nbits: u32 = 0;
    let mut bail: u64 = 0;
    for c in &mut chunks {
        let x = u64::from_le_bytes(c.try_into().expect("chunks_exact yields 8 bytes"));
        bail |= swar_bail(x);
        acc |= movemask(swar_word(x)) << nbits;
        nbits += 8;
        if nbits == 64 {
            bm.push(acc);
            acc = 0;
            nbits = 0;
        }
    }
    let rem = chunks.remainder();
    if !rem.is_empty() {
        // Zero padding is inert: 0x00 is neither a word byte nor a bail byte.
        let mut buf = [0u8; 8];
        buf[..rem.len()].copy_from_slice(rem);
        let x = u64::from_le_bytes(buf);
        bail |= swar_bail(x);
        acc |= movemask(swar_word(x)) << nbits;
        nbits += 8;
    }
    if nbits > 0 {
        bm.push(acc);
    }
    bail == 0
}

/// Pass 2: walks the word bitmap, handing each maximal run of one-bits to `f`
/// as a byte range. Runs are exactly `WordTokenizer`'s tokens on certified
/// input, including runs that span bitmap words or reach the end of text.
#[inline]
fn scan_runs(len: usize, bm: &[u64], mut f: impl FnMut(usize, usize)) {
    let n = bm.len();
    let mut wi = 0usize;
    let mut cur = if n == 0 { 0 } else { bm[0] };
    loop {
        while cur == 0 {
            wi += 1;
            if wi >= n {
                return;
            }
            cur = bm[wi];
        }
        let tz = cur.trailing_zeros();
        let start = wi * 64 + tz as usize;
        // Fill the bits below the run, then find the first zero at/after it.
        let filled = cur | (1u64 << tz).wrapping_sub(1);
        let mut nz = (!filled).trailing_zeros();
        let mut end_wi = wi;
        while nz == 64 {
            end_wi += 1;
            if end_wi >= n {
                // The run extends to the very end of the bitmap.
                f(start, len.min(end_wi * 64));
                return;
            }
            nz = (!bm[end_wi]).trailing_zeros();
        }
        let end = end_wi * 64 + nz as usize;
        f(start, end);
        wi = end_wi;
        // Clear consumed bits (bit `nz` itself is already zero).
        cur = if nz >= 63 {
            0
        } else {
            bm[end_wi] & !((1u64 << (nz + 1)) - 1)
        };
    }
}

// ---------------------------------------------------------------------------
// The per-document builder
// ---------------------------------------------------------------------------

/// One document's accumulation state — the moral equivalent of a
/// [`BuiltDocument`] mid-`observe`, with the hash index replaced by the dense
/// slot table and the derived index built once at the end.
struct DocBuilder<'a> {
    interner: &'a mut Interner,
    slots: &'a mut Vec<u32>,
    touched: &'a mut Vec<TermId>,
    entries: Vec<(TermId, f64)>,
    key: DocKey,
}

impl DocBuilder<'_> {
    /// Applies one token occurrence, given as a byte range of `text`.
    ///
    /// Replicates the reference reduce body exactly; every branch below has a
    /// counterpart in [`BuiltDocument::observe`], reordered only where the
    /// reordering is invisible (flags are per-spelling constants, and the
    /// stop-word answer is stable within one document — see the module doc).
    #[inline]
    fn observe_range(&mut self, text: &str, start: usize, end: usize) {
        let bytes = text.as_bytes();
        let len = end - start;
        debug_assert!(len >= 1, "tokenizer runs are never empty");
        let (id, flags) = if len <= 8 {
            let key = if start + 8 <= bytes.len() {
                // One unaligned load, masked down to the token's bytes —
                // cheaper than a length-dependent copy when the tail of the
                // buffer is in bounds, which it almost always is.
                let word =
                    u64::from_le_bytes(bytes[start..start + 8].try_into().expect("eight bytes"));
                word & (u64::MAX >> (64 - 8 * len as u32))
            } else {
                Interner::pack_short(&bytes[start..end])
            };
            self.interner.intern_short(key, len)
        } else {
            self.interner.intern_long(&text[start..end])
        };
        let idx = id as usize;
        if idx >= self.slots.len() {
            self.slots.resize(idx + 1, 0);
        }
        let s = self.slots[idx];
        if s == 0 {
            self.first_occurrence(id, flags, &text[start..end]);
            return;
        }
        let stop = s & SLOT_STOP != 0;
        if flags & (TERM_KEY_PROPERTY | TERM_PROTO_PROPERTY) != 0 {
            // `__proto__` is dropped; `__key` bumps the key when unfiltered.
            if flags & TERM_KEY_PROPERTY != 0 && !stop {
                self.bump_key();
            }
            return;
        }
        if stop {
            // A stop-worded term never counts — its issue-#119 zero slot (if
            // it names a prototype method) was already left behind on first
            // occurrence.
            return;
        }
        // Every non-stop, non-special term has an entry after its first
        // occurrence, so the slot's index part is nonzero here.
        let e = (s & SLOT_INDEX) as usize;
        let v = &mut self.entries[e - 1].1;
        // `document[term] ? document[term] + 1 : 1` — a zeroed slot restarts
        // at 1 rather than becoming 1 by increment.
        *v = if *v == 0.0 || v.is_nan() {
            1.0
        } else {
            *v + 1.0
        };
    }

    /// The first occurrence of a term in this document: resolves its
    /// stop-word status (fresh per document) and seeds its slot.
    fn first_occurrence(&mut self, id: TermId, flags: u8, term: &str) {
        let stop = globals::is_stopword(term);
        self.touched.push(id);
        let mut s = SLOT_TOUCHED | if stop { SLOT_STOP } else { 0 };
        if flags & (TERM_KEY_PROPERTY | TERM_PROTO_PROPERTY) != 0 {
            self.slots[id as usize] = s;
            if flags & TERM_KEY_PROPERTY != 0 && !stop {
                self.bump_key();
            }
            return;
        }
        if flags & TERM_PROTO_METHOD != 0 {
            // Issue #119: the inherited method is zeroed before the stop-word
            // test, so even a filtered occurrence leaves a `0` entry; an
            // unfiltered one restarts the zeroed slot at 1 in the same step.
            s |= self.push_entry(id, if stop { 0.0 } else { 1.0 });
        } else if !stop {
            s |= self.push_entry(id, 1.0);
        }
        self.slots[id as usize] = s;
    }

    /// Appends an entry, returning its slot-encoded index (index + 1).
    fn push_entry(&mut self, id: TermId, count: f64) -> u32 {
        let i = self.entries.len();
        // The slot encoding keeps 30 bits for the index; at 12+ bytes per
        // entry, a document would need gigabytes of distinct terms to get
        // anywhere near it.
        assert!(
            i < SLOT_INDEX as usize,
            "documents hold fewer than 2^30 distinct terms"
        );
        self.entries.push((id, count));
        i as u32 + 1
    }

    /// The reference `document.__key = document.__key ? document.__key + 1 : 1`.
    fn bump_key(&mut self) {
        self.key = if self.key.is_truthy() {
            self.key.plus_one()
        } else {
            DocKey::Num(1.0)
        };
    }
}

/// Builds a text document through the fast path.
///
/// The caller (`build_document`) has already established that the
/// process-global tokenizer is the default `WordTokenizer`; everything else —
/// lowercasing, tokenizer choice per input class, stop-word freshness — is
/// decided here:
///
/// 1. Certified lowercase ASCII: bitmap-driven scan over the borrowed text
///    (the exact inputs where `lowercase_units` borrows).
/// 2. Bail on uppercase/non-ASCII: `to_lowercase()` — the same owned string
///    `lowercase_units` produces — then retry the bitmap.
/// 3. Still non-ASCII: the real `WordTokenizer` walks the lowered text
///    char-exactly, feeding the same fast interner and counter.
pub(crate) fn build_text(
    interner: &mut Interner,
    scratch: &mut FastScratch,
    text: &str,
    key: DocKey,
) -> BuiltDocument {
    let FastScratch {
        bitmap,
        slots,
        touched,
    } = scratch;
    let mut b = DocBuilder {
        interner,
        slots,
        touched,
        entries: Vec::new(),
        key,
    };
    if build_bitmap(text.as_bytes(), bitmap) {
        scan_runs(text.len(), bitmap, |s, e| b.observe_range(text, s, e));
    } else {
        let lowered = text.to_lowercase();
        if build_bitmap(lowered.as_bytes(), bitmap) {
            scan_runs(lowered.len(), bitmap, |s, e| {
                b.observe_range(&lowered, s, e)
            });
        } else {
            // Char-exact fallback: identical scanning to the reference loop.
            let base = lowered.as_ptr() as usize;
            let tokens = WordTokenizer::new()
                .tokens(&lowered)
                .expect("WordTokenizer splitting mode always matches");
            for term in tokens {
                let start = term.as_ptr() as usize - base;
                b.observe_range(&lowered, start, start + term.len());
            }
        }
    }
    let DocBuilder {
        entries,
        key,
        slots,
        touched,
        ..
    } = b;
    for &id in touched.iter() {
        slots[id as usize] = 0;
    }
    touched.clear();
    BuiltDocument::from_parts(key, entries)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Tokens per the bitmap path, or `None` when the input bails.
    fn bitmap_tokens(text: &str) -> Option<Vec<String>> {
        let mut bm = Vec::new();
        if !build_bitmap(text.as_bytes(), &mut bm) {
            return None;
        }
        let mut out = Vec::new();
        scan_runs(text.len(), &bm, |s, e| out.push(text[s..e].to_owned()));
        Some(out)
    }

    fn reference_tokens(text: &str) -> Vec<String> {
        WordTokenizer::new()
            .tokens(text)
            .expect("splitting mode")
            .map(str::to_owned)
            .collect()
    }

    #[test]
    fn every_ascii_byte_classifies_like_word_tokenizer() {
        // Exhaustive over the byte the bitmap can certify: for each ASCII
        // byte, embedded between word characters, the split decision must
        // match `WordTokenizer`, and the bail set must be exactly the bytes
        // `lowercase_units` would rewrite for ASCII input.
        for b in 0u8..=0x7F {
            let c = b as char;
            let s = format!("aa{c}bb");
            match bitmap_tokens(&s) {
                None => assert!(c.is_ascii_uppercase(), "byte {b:#04x} bailed unexpectedly"),
                Some(mine) => {
                    assert!(!c.is_ascii_uppercase(), "byte {b:#04x} should bail");
                    assert_eq!(mine, reference_tokens(&s), "byte {b:#04x} split diverged");
                }
            }
        }
    }

    #[test]
    fn non_ascii_always_bails() {
        for s in ["naïve", "ёлка", "日本語", "😀abc😀", "𝕳𝖊𝖑𝖑𝖔", "a\u{80}b"]
        {
            assert!(bitmap_tokens(s).is_none(), "{s:?} must bail");
        }
    }

    #[test]
    fn random_ascii_strings_tokenize_identically() {
        // Property test against the real tokenizer, mirroring the lab
        // verification of this exact algorithm (20,000 strings there).
        let mut state = 0x1319_8A2E_0370_7344u64;
        let mut xorshift = move || {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state
        };
        let mut checked = 0u32;
        for _ in 0..20_000 {
            let len = (xorshift() % 200) as usize;
            let s: String = (0..len)
                .map(|_| {
                    let b = (xorshift() % 128) as u8;
                    // Uppercase bails by design; remap it to stay on the path
                    // under test.
                    (if b.is_ascii_uppercase() { b'x' } else { b }) as char
                })
                .collect();
            let mine = bitmap_tokens(&s).expect("no bail bytes were generated");
            assert_eq!(mine, reference_tokens(&s), "diverged on {s:?}");
            checked += 1;
        }
        assert_eq!(checked, 20_000);
    }

    #[test]
    fn runs_crossing_bitmap_word_boundaries() {
        // Maximal runs that end exactly at, straddle, and overrun the 64-bit
        // bitmap words, including a run that reaches the end of the text —
        // the paths `scan_runs` special-cases.
        for n in [1usize, 7, 8, 9, 63, 64, 65, 127, 128, 129, 200, 1000] {
            let word = "a".repeat(n);
            for text in [
                word.clone(),
                format!(" {word}"),
                format!("{word} "),
                format!("x {word} y"),
                format!("{word}.{word}"),
            ] {
                assert_eq!(
                    bitmap_tokens(&text).expect("pure lowercase ASCII"),
                    reference_tokens(&text),
                    "diverged at n={n} on {text:?}"
                );
            }
        }
    }

    #[test]
    fn empty_and_separator_only_inputs_yield_no_tokens() {
        for s in ["", " ", "  .,;  ", "\n\t"] {
            assert_eq!(bitmap_tokens(s).expect("no bail"), Vec::<String>::new());
        }
    }
}
