//! `CORRECTNESS BEFORE PERFORMANCE` for `benches/tokenizers.rs`.
//!
//! `benches/tokenizers.rs`'s module doc comment narrows its input domain to
//! punctuation-free, single-ASCII-space-joined lowercase-letter text —
//! exactly the domain where Verbora's, tantivy's and Hugging Face
//! `tokenizers`' otherwise-divergent character classes and whitespace
//! definitions are documented to coincide. This file proves that claim: on
//! that domain the implementations draw the exact same token boundaries —
//! not merely the same token *count* — before any timing number from that
//! file is trusted.
//!
//! # What the text-shaping migration did to this file
//!
//! `docs/design/text-shaping-contract.md` deleted `RegexpTokenizer`,
//! `Pattern`, `AggressiveTokenizer` and its fifteen language variants, and
//! the `Tokenize` trait (§3.4). The tests that exercised them are not
//! re-pointed at a lookalike:
//!
//! * The `whitespace_tokenization` tests are **replaced**, not retargeted.
//!   Verbora performs no whitespace or regex tokenization at any API, so
//!   there is no cross-implementation claim left to verify — but the
//!   capability contract §3.4 names as the replacement, `SegmentTokenizer`,
//!   has a guarantee no competitor here offers and nothing was pinning it in
//!   this harness: concatenating its segments reproduces the input exactly.
//!   [`segment_tokenizer_concatenation_reproduces_the_input`] and
//!   [`word_tokenizer_is_a_subsequence_of_segment_tokenizer`] pin
//!   **Verbora's own contract** in its place, per this round's instruction to
//!   replace a comparison that stopped being meaningful rather than delete
//!   the coverage outright.
//! * The `aggressive_tokenization_en` tests are **deleted**.
//!   `AggressiveTokenizer` is gone and the comparison it made
//!   (`unicode_words()` agreement on the narrowed domain) is now carried by
//!   [`WordTokenizer`] itself, which *is* `str::unicode_words()`.
//!
//! # Two questions, kept apart
//!
//! 1. **Does Verbora honour its own contract?** Tokens are substrings,
//!    `SegmentTokenizer` concatenates to the input, `WordTokenizer` is a
//!    subsequence of it, `SentenceTokenizer` does not trim. None of these
//!    depends on a competitor, and the last one is a *behaviour change*
//!    (§3.1 removed sentence trimming) that would otherwise be visible only
//!    as a mysteriously-failing agreement test.
//! 2. **Is the benchmark comparing like with like?** Cross-implementation
//!    agreement on the narrowed domains, along three axes the pre-migration
//!    file already established and this one keeps: seeded random sweeps,
//!    ground-truth agreement against the constructed word/sentence sequence
//!    (two implementations sharing a bug would pass a pure cross-check), and
//!    byte-offset agreement (equal token text could in principle come from
//!    different positions in repetitive input).

use segtok::segmenter::{SegmentConfig, split_single};
use tantivy::tokenizer::{
    SimpleTokenizer, TokenStream as TantivyTokenStream, Tokenizer as TantivyTokenizer,
};
use tokenizers::pre_tokenizers::whitespace::Whitespace;
use tokenizers::{OffsetReferential, OffsetType, PreTokenizedString, PreTokenizer};
use unicode_segmentation::UnicodeSegmentation;
use verbora_tokenizers::{BorrowingTokenizer, SegmentTokenizer, SentenceTokenizer, WordTokenizer};

/// A punctuation-free document: real words, single ASCII spaces, no digits,
/// no underscores, no non-ASCII whitespace — independent of
/// `benches/data/words.json` so this test needs no generated fixture.
fn narrowed_domain_text() -> String {
    let words = [
        "the",
        "quick",
        "brown",
        "fox",
        "jumps",
        "over",
        "the",
        "lazy",
        "dog",
        "while",
        "a",
        "second",
        "sentence",
        "worth",
        "of",
        "plain",
        "ascii",
        "words",
        "keeps",
        "the",
        "run",
        "long",
        "enough",
        "to",
        "exercise",
        "more",
        "than",
        "one",
        "internal",
        "buffer",
        "growth",
        "and",
        "a",
        "third",
        "pass",
        "for",
        "good",
        "measure",
        "across",
        "many",
        "distinct",
        "boundaries",
    ];
    words
        .iter()
        .cycle()
        .take(400)
        .cloned()
        .collect::<Vec<_>>()
        .join(" ")
}

fn tantivy_tokens(tok: &mut impl TantivyTokenizer, text: &str) -> Vec<String> {
    let mut stream = tok.token_stream(text);
    let mut out = Vec::new();
    while stream.advance() {
        out.push(stream.token().text.clone());
    }
    out
}

fn hf_tokens(pt: &impl PreTokenizer, text: &str) -> Vec<String> {
    let mut pretokenized = PreTokenizedString::from(text);
    pt.pre_tokenize(&mut pretokenized)
        .expect("Whitespace::pre_tokenize never fails");
    pretokenized
        .get_splits(OffsetReferential::Original, OffsetType::Byte)
        .into_iter()
        .map(|(s, _, _)| s.to_owned())
        .collect()
}

/// `WordTokenizer` tokens as owned strings.
fn verbora_word_tokens(tok: &WordTokenizer, text: &str) -> Vec<String> {
    tok.tokenize_borrowed(text)
        .into_iter()
        .map(str::to_owned)
        .collect()
}

// ---------------------------------------------------------------------------
// 1. Verbora against its own contract — no competitor involved
// ---------------------------------------------------------------------------

/// Contract §3.1, guarantee 1: `SegmentTokenizer.tokens(t).collect::<String>()
/// == t`, for every `t`. This is what a highlighter, a re-assembler or an
/// offset consumer needs and what no filtered tokenizer can offer — the
/// capability §3.4 names as `RegexpTokenizer`'s replacement, and the reason
/// the deleted `whitespace_tokenization` coverage is replaced by a
/// Verbora-contract test rather than dropped.
#[test]
fn segment_tokenizer_concatenation_reproduces_the_input() {
    let mut rng = SplitMix64(0x7031_C0DE_0101);
    let mut corpus: Vec<String> = vec![
        String::new(),
        " ".to_owned(),
        "   ".to_owned(),
        "one".to_owned(),
        "one two".to_owned(),
        narrowed_domain_text(),
        narrowed_sentence_domain_text(),
        // Deliberately *outside* the narrowed benchmark domain: the guarantee
        // is total, so it is checked on text no competitor row ever sees.
        "well-known and/or don't 3.14 1,000 node_js".to_owned(),
        "café naïve Äpfel привет, мир 日本語 すもももももも".to_owned(),
        "a😀b\u{FEFF}\u{85}\t\r\n\u{2028}".to_owned(),
    ];
    for n_words in [1, 2, 17, 129] {
        corpus.push(random_narrowed_document(&mut rng, n_words).0);
    }

    for text in &corpus {
        let joined: String = SegmentTokenizer.tokens(text).collect();
        assert_eq!(&joined, text, "concatenation must reproduce {text:?}");
        assert!(
            SegmentTokenizer.tokens(text).all(|t| !t.is_empty()),
            "no tokenizer yields the empty string ({text:?})"
        );
        // Every token is a contiguous slice of the input, at strictly
        // increasing non-overlapping byte ranges (guarantee 2).
        let mut expected_start = 0usize;
        for token in SegmentTokenizer.tokens(text) {
            let (start, end) = span_in(text, token);
            assert_eq!(start, expected_start, "segments must be contiguous");
            expected_start = end;
        }
        assert_eq!(expected_start, text.len());
    }
}

/// Contract §3.1, guarantee 4: `WordTokenizer.tokens(t)` is a subsequence of
/// `SegmentTokenizer.tokens(t)` with equal pointer identity for corresponding
/// tokens — the filter is a filter, not a second segmentation.
#[test]
fn word_tokenizer_is_a_subsequence_of_segment_tokenizer() {
    for text in [
        narrowed_domain_text(),
        "well-known and/or don't 3.14 1,000 node_js".to_owned(),
        "café naïve Äpfel привет, мир 日本語".to_owned(),
        "a😀b".to_owned(),
        String::new(),
    ] {
        let segments: Vec<&str> = SegmentTokenizer.tokens(&text).collect();
        let words: Vec<&str> = WordTokenizer.tokens(&text).collect();

        let mut seg = segments.iter();
        for word in &words {
            let found =
                seg.find(|s| std::ptr::eq(s.as_ptr(), word.as_ptr()) && s.len() == word.len());
            assert!(
                found.is_some(),
                "word token {word:?} must be one of the segments, by pointer identity ({text:?})"
            );
        }
        assert!(words.len() <= segments.len());
    }
}

/// Contract §3.1: sentences are **not** trimmed — a sentence includes its own
/// trailing whitespace, so concatenation reproduces the input exactly, and
/// `tokens("   ")` is `["   "]` rather than the pre-migration `[""]`, a token
/// that occurred nowhere in the input.
///
/// This is a behaviour change, and pinning it here is what keeps the
/// competitor comparisons below honest: `segtok` trims and Verbora no longer
/// does, so every cross-check that compares against `segtok` normalizes for
/// that convention deliberately rather than by accident.
#[test]
fn sentence_tokenizer_does_not_trim() {
    let t = SentenceTokenizer::new();

    assert_eq!(t.tokenize_borrowed("   "), ["   "]);
    assert_eq!(t.tokenize_borrowed(""), Vec::<&str>::new());
    assert_eq!(
        t.tokenize_borrowed("One. Two."),
        ["One. ", "Two."],
        "the inter-sentence space belongs to the sentence that precedes it"
    );

    for text in [
        narrowed_sentence_domain_text(),
        "One.  Two.\nThree. ".to_owned(),
        narrowed_domain_text(),
    ] {
        let joined: String = t.tokens(&text).collect();
        assert_eq!(joined, text, "concatenation must reproduce {text:?}");
    }
}

/// With no abbreviations configured, `SentenceTokenizer` is exactly the
/// untailored UAX #29 §5 segmentation — not approximately, and not only on
/// the narrowed domain. Asserted against `split_sentence_bounds()` untrimmed,
/// which is the strongest form this claim can take and the reason the
/// `sentence_tokenization_wrapper_overhead` bench group exists.
#[test]
fn untailored_sentence_tokenizer_is_exactly_uax29_sentence_bounds() {
    let t = SentenceTokenizer::new();
    let mut rng = SplitMix64(0x7031_C0DE_0102);
    let mut corpus = vec![
        String::new(),
        "   ".to_owned(),
        "One. Two. Three.".to_owned(),
        "Mr. Fox jumped. Then he ran.".to_owned(),
        "No punctuation at all".to_owned(),
        "Ends without a period".to_owned(),
        narrowed_sentence_domain_text(),
        "日本語。 これは 文です。".to_owned(),
        "A\u{85}B\u{2029}C".to_owned(),
    ];
    for i in 0..12 {
        corpus.push(random_sentence_document(&mut rng, 1 + i, 1 + (i % 7)));
    }

    for text in &corpus {
        let verbora: Vec<&str> = t.tokens(text).collect();
        let bounds: Vec<&str> = text.split_sentence_bounds().collect();
        assert_eq!(
            verbora, bounds,
            "untailored tokenizer vs UAX #29 ({text:?})"
        );
    }
}

/// The abbreviation tailoring — the one thing `SentenceTokenizer` does that
/// `split_sentence_bounds()` does not — including the over-suppression the
/// contract documents by worked example (§3.1) rather than patching.
#[test]
fn abbreviation_tailoring_matches_the_contracts_worked_examples() {
    use verbora_tokenizers::AbbreviationError;

    let t = SentenceTokenizer::with_abbreviations(["Dr."]).expect("non-empty");
    assert_eq!(
        t.tokenize_borrowed("Dr. Fox went home. Then he slept."),
        ["Dr. Fox went home. ", "Then he slept."]
    );

    // Suffix matching, so it can over-suppress — the contract's own example.
    let t = SentenceTokenizer::with_abbreviations(["no."]).expect("non-empty");
    assert_eq!(
        t.tokenize_borrowed("Visit the casino. Then leave."),
        ["Visit the casino. Then leave."],
        "\"casino.\" ends with \"no.\", so the boundary is suppressed"
    );

    // Case-sensitive: the capitalized form does not match the lowercase entry.
    let t = SentenceTokenizer::with_abbreviations(["No."]).expect("non-empty");
    assert_eq!(
        t.tokenize_borrowed("Visit the casino. Then leave."),
        ["Visit the casino. ", "Then leave."]
    );

    // An empty abbreviation is unrepresentable, not merely discouraged.
    assert_eq!(
        SentenceTokenizer::with_abbreviations(["Dr.", ""]),
        Err(AbbreviationError::Empty { index: 1 })
    );
}

// ---------------------------------------------------------------------------
// 2. Cross-implementation agreement on the narrowed word-boundary domain
// ---------------------------------------------------------------------------

/// The `word_tokenization` group: `verbora`'s `WordTokenizer`,
/// `tantivy::SimpleTokenizer`, Hugging Face `Whitespace`.
#[test]
fn word_tokenization_agrees_on_narrowed_domain() {
    let text = narrowed_domain_text();

    let verbora_tokens = verbora_word_tokens(&WordTokenizer, &text);

    let mut tantivy_tok = SimpleTokenizer::default();
    let tantivy_tokens = tantivy_tokens(&mut tantivy_tok, &text);

    let hf_tokens = hf_tokens(&Whitespace, &text);

    assert_eq!(
        verbora_tokens, tantivy_tokens,
        "verbora WordTokenizer and tantivy::SimpleTokenizer must draw identical token \
         boundaries on punctuation-free single-space-joined ASCII text"
    );
    assert_eq!(
        verbora_tokens, hf_tokens,
        "verbora WordTokenizer and Hugging Face Whitespace must draw identical token \
         boundaries on punctuation-free single-space-joined ASCII text"
    );
    assert!(verbora_tokens.len() > 300);
}

/// The `word_tokenization_wrapper_overhead` group. `WordTokenizer::tokens` is
/// literally `str::unicode_words()`, so exact agreement there is a structural
/// fact rather than a coincidence and is asserted as such — if it ever fails,
/// the wrapper has grown behaviour and that group's own doc comment is wrong.
/// `split_word_bounds()` (whitespace spans filtered) is the lower-level API a
/// caller would have to drive by hand.
#[test]
fn word_tokenization_agrees_with_unicode_segmentation_on_narrowed_domain() {
    let text = narrowed_domain_text();

    let verbora_tokens = verbora_word_tokens(&WordTokenizer, &text);

    let unicode_words: Vec<String> = text.unicode_words().map(str::to_owned).collect();
    assert_eq!(
        verbora_tokens, unicode_words,
        "WordTokenizer::tokens IS str::unicode_words(); the wrapper-overhead group's \
         claim depends on this being exact"
    );

    let unicode_bounds_words: Vec<String> = text
        .split_word_bounds()
        .filter(|s| !s.chars().all(char::is_whitespace))
        .map(str::to_owned)
        .collect();
    assert_eq!(
        verbora_tokens, unicode_bounds_words,
        "verbora WordTokenizer and unicode-segmentation's split_word_bounds() (whitespace \
         spans filtered out) must yield identical word tokens on the same narrowed domain"
    );

    assert!(verbora_tokens.len() > 300);
}

/// The exact agreement above must hold on *arbitrary* text, not only the
/// narrowed domain — `WordTokenizer` adds no rule of its own. Checked on the
/// inputs contract §3.1's migration table names, i.e. precisely where the
/// deleted `AggressiveTokenizer` and the standard disagreed.
#[test]
fn word_tokenizer_equals_unicode_words_off_the_narrowed_domain_too() {
    for text in [
        "well-known",
        "and/or",
        "don't",
        "3.14",
        "1,000",
        "node_js",
        "Äpfel",
        "a×b÷c",
        "привет, мир",
        "café naïve",
        "日本語",
        "すもももももも",
        "a😀b",
        "",
        "   ",
    ] {
        let verbora: Vec<&str> = WordTokenizer.tokens(text).collect();
        let unicode: Vec<&str> = text.unicode_words().collect();
        assert_eq!(verbora, unicode, "{text:?}");
    }
}

// ---------------------------------------------------------------------------
// 3. Seeded random sweeps, ground truth, byte offsets
// ---------------------------------------------------------------------------

/// SplitMix64 (Steele, Lea & Flood 2014) — a tiny, dependency-free,
/// fully deterministic generator, so every sweep below reproduces
/// bit-for-bit on any machine and a reported failure's inputs can be
/// regenerated from the seed alone.
struct SplitMix64(u64);

impl SplitMix64 {
    fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// A uniform-enough index in `0..n` (modulo bias is irrelevant here:
    /// these draws pick word lengths and letters, not statistics).
    fn below(&mut self, n: usize) -> usize {
        (self.next_u64() % n as u64) as usize
    }
}

/// A random lowercase ASCII `[a-z]` word of length `min_len..=max_len` —
/// exactly the character class the narrowed word-boundary domain permits.
fn random_ascii_word(rng: &mut SplitMix64, min_len: usize, max_len: usize) -> String {
    let len = min_len + rng.below(max_len - min_len + 1);
    (0..len)
        .map(|_| char::from(b'a' + rng.below(26) as u8))
        .collect()
}

/// A random in-domain document: `n_words` lowercase ASCII words (lengths
/// 1..=14), joined by exactly one U+0020 space. Returns the document *and*
/// the word sequence it was built from, so callers can assert ground truth,
/// not just cross-agreement.
fn random_narrowed_document(rng: &mut SplitMix64, n_words: usize) -> (String, Vec<String>) {
    let words: Vec<String> = (0..n_words)
        .map(|_| random_ascii_word(rng, 1, 14))
        .collect();
    (words.join(" "), words)
}

/// Word counts for one sweep: the two degenerate shapes first (a single
/// word — no separator at all — and a two-word document with exactly one
/// boundary), then `docs - 2` random sizes in `3..=402`.
fn sweep_word_counts(rng: &mut SplitMix64, docs: usize) -> Vec<usize> {
    let mut counts = vec![1, 2];
    counts.extend((2..docs).map(|_| 3 + rng.below(400)));
    counts
}

/// Seeded random sweep of the `word_tokenization` group — 48 documents
/// spanning 1..=402 words. Beyond the three-way agreement the fixed fixture
/// proves, every document's tokens are also checked against the exact word
/// sequence the document was constructed from — ground truth no pure
/// cross-check can provide.
#[test]
fn word_tokenization_agrees_on_seeded_random_documents() {
    let mut rng = SplitMix64(0x7031_C0DE_0002);
    let verbora = WordTokenizer;
    let mut tantivy_tok = SimpleTokenizer::default();

    for n_words in sweep_word_counts(&mut rng, 48) {
        let (text, expected) = random_narrowed_document(&mut rng, n_words);

        let verbora_tokens = verbora_word_tokens(&verbora, &text);
        assert_eq!(
            verbora_tokens, expected,
            "verbora WordTokenizer must reproduce the exact word sequence a \
             single-space-joined document was constructed from (n_words={n_words})"
        );
        assert_eq!(
            verbora_tokens,
            tantivy_tokens(&mut tantivy_tok, &text),
            "verbora and tantivy::SimpleTokenizer must agree on random in-domain \
             documents (n_words={n_words})"
        );
        assert_eq!(
            verbora_tokens,
            hf_tokens(&Whitespace, &text),
            "verbora and Hugging Face Whitespace must agree on random in-domain \
             documents (n_words={n_words})"
        );
    }
}

/// Seeded random sweep of the `word_tokenization_wrapper_overhead` group:
/// `WordTokenizer` against `unicode_words()` and the whitespace-filtered
/// `split_word_bounds()` (the same documented filter the bench times), plus
/// the ground-truth word sequence.
#[test]
fn unicode_segmentation_words_agree_on_seeded_random_documents() {
    let mut rng = SplitMix64(0x7031_C0DE_0003);
    let verbora = WordTokenizer;

    for n_words in sweep_word_counts(&mut rng, 48) {
        let (text, expected) = random_narrowed_document(&mut rng, n_words);

        let verbora_tokens = verbora_word_tokens(&verbora, &text);
        assert_eq!(
            verbora_tokens, expected,
            "verbora WordTokenizer must reproduce the constructed word sequence \
             (n_words={n_words})"
        );

        let unicode_words: Vec<String> = text.unicode_words().map(str::to_owned).collect();
        assert_eq!(
            verbora_tokens, unicode_words,
            "verbora and unicode_words() must agree on random in-domain documents \
             (n_words={n_words})"
        );

        let unicode_bounds_words: Vec<String> = text
            .split_word_bounds()
            .filter(|s| !s.chars().all(char::is_whitespace))
            .map(str::to_owned)
            .collect();
        assert_eq!(
            verbora_tokens, unicode_bounds_words,
            "verbora and split_word_bounds() (whitespace spans filtered out) must agree \
             on random in-domain documents (n_words={n_words})"
        );
    }
}

/// Byte span of `token` inside `text`, by pointer arithmetic — sound because
/// every verbora tokenizer returns subslices borrowed from the input
/// (contract §1, "Tokens are substrings"), which the containment assert
/// re-verifies.
fn span_in(text: &str, token: &str) -> (usize, usize) {
    let start = (token.as_ptr() as usize)
        .checked_sub(text.as_ptr() as usize)
        .expect("token must borrow from `text`");
    let end = start + token.len();
    assert!(end <= text.len(), "token must lie inside `text`");
    (start, end)
}

/// tantivy token byte offsets (`Token::offset_from`/`offset_to`).
fn tantivy_spans(tok: &mut impl TantivyTokenizer, text: &str) -> Vec<(usize, usize)> {
    let mut stream = tok.token_stream(text);
    let mut out = Vec::new();
    while stream.advance() {
        let t = stream.token();
        out.push((t.offset_from, t.offset_to));
    }
    out
}

/// Hugging Face split byte offsets, in the same
/// `OffsetReferential::Original`/`OffsetType::Byte` terms the token-text
/// helper above uses.
fn hf_spans(pt: &impl PreTokenizer, text: &str) -> Vec<(usize, usize)> {
    let mut pretokenized = PreTokenizedString::from(text);
    pt.pre_tokenize(&mut pretokenized)
        .expect("Whitespace::pre_tokenize never fails");
    pretokenized
        .get_splits(OffsetReferential::Original, OffsetType::Byte)
        .into_iter()
        .map(|(_, (start, end), _)| (start, end))
        .collect()
}

/// The implementations must place every token at the same byte *positions*,
/// not merely produce the same token *text*. On cyclic input like these
/// fixtures (and the bench's own cycled corpus), equal text sequences could
/// in principle be produced from different positions; offsets close that
/// gap. Verbora's offsets come from pointer arithmetic on its borrowed
/// `&str` tokens — which is only possible *because* contract §1 guarantees
/// every token is a substring — tantivy's from `Token::offset_from`/
/// `offset_to`, Hugging Face's from `get_splits(.., OffsetType::Byte)`.
#[test]
fn word_boundary_byte_offsets_agree_on_narrowed_domain() {
    let mut rng = SplitMix64(0x7031_C0DE_0005);
    let fixed = narrowed_domain_text();
    let (random_a, _) = random_narrowed_document(&mut rng, 257);
    let (random_b, _) = random_narrowed_document(&mut rng, 31);

    for text in [&fixed, &random_a, &random_b] {
        let verbora_word_spans: Vec<(usize, usize)> = WordTokenizer
            .tokens(text)
            .map(|t| span_in(text, t))
            .collect();
        assert_eq!(
            verbora_word_spans,
            tantivy_spans(&mut SimpleTokenizer::default(), text),
            "verbora WordTokenizer and tantivy::SimpleTokenizer must place every token \
             at identical byte offsets"
        );
        assert_eq!(
            verbora_word_spans,
            hf_spans(&Whitespace, text),
            "verbora WordTokenizer and Hugging Face Whitespace must place every token \
             at identical byte offsets"
        );
        assert!(!verbora_word_spans.is_empty());
    }
}

/// Fixed edge shapes of the word-boundary domain, checked across every
/// benchmarked word-boundary implementation at once (verbora, tantivy,
/// Hugging Face, unicode-segmentation ×2) plus ground truth: a single word
/// (no separator anywhere), exactly one boundary, minimum-length and
/// corpus-maximum (14-char) words, alternating extremes, and one
/// pathologically long single word — all still lowercase ASCII words joined
/// by single spaces, i.e. squarely inside the documented narrowed domain.
#[test]
fn word_boundary_edge_shapes_agree_on_narrowed_domain() {
    let long_word = "z".repeat(4096);
    let three: Vec<String> = std::iter::repeat_n("fox".to_owned(), 400).collect();
    let fourteen: Vec<String> = std::iter::repeat_n("wordfourteench".to_owned(), 200).collect();
    let alternating: Vec<String> = ["a", "wordfourteench"]
        .iter()
        .cycle()
        .take(301)
        .map(|&w| w.to_owned())
        .collect();

    let cases: Vec<Vec<String>> = vec![
        vec!["boundary".to_owned()],
        vec!["two".to_owned(), "words".to_owned()],
        three,
        fourteen,
        alternating,
        vec![long_word],
    ];

    for expected in cases {
        let text = expected.join(" ");
        let label = format!("{} word(s), {} byte(s)", expected.len(), text.len());

        assert_eq!(
            verbora_word_tokens(&WordTokenizer, &text),
            expected,
            "verbora WordTokenizer must reproduce the constructed words ({label})"
        );
        assert_eq!(
            tantivy_tokens(&mut SimpleTokenizer::default(), &text),
            expected,
            "tantivy::SimpleTokenizer must agree on the edge shape ({label})"
        );
        assert_eq!(
            hf_tokens(&Whitespace, &text),
            expected,
            "Hugging Face Whitespace must agree on the edge shape ({label})"
        );
        assert_eq!(
            text.unicode_words().map(str::to_owned).collect::<Vec<_>>(),
            expected,
            "unicode_words() must agree on the edge shape ({label})"
        );
        assert_eq!(
            text.split_word_bounds()
                .filter(|s| !s.chars().all(char::is_whitespace))
                .map(str::to_owned)
                .collect::<Vec<_>>(),
            expected,
            "split_word_bounds() (whitespace spans filtered out) must agree on the edge \
             shape ({label})"
        );
    }
}

// ---------------------------------------------------------------------------
// 4. Sentence boundaries
// ---------------------------------------------------------------------------

/// A short document of plain declarative sentences: capitalized first word,
/// lowercase rest, exactly one `.` per sentence, exactly one space between
/// sentences, no digits/quotes/brackets/abbreviations/newlines — the
/// narrowed sentence-boundary domain `benches/tokenizers.rs`'s own
/// `sentence_prose` builds.
fn narrowed_sentence_domain_text() -> String {
    let sentences = [
        "Quick brown fox jumps over lazy dog",
        "Second sentence keeps its own five words",
        "Third example uses only plain letters",
        "Fourth one runs long enough to matter",
        "Fifth closes out this short document",
    ];
    sentences
        .iter()
        .cycle()
        .take(40)
        .map(|s| {
            let mut chars = s.chars();
            let first = chars.next().expect("non-empty sentence");
            format!("{}{}.", first, chars.as_str())
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Asserts the `sentence_tokenization` group's implementations agree on
/// `text`.
///
/// Verbora no longer trims (contract §3.1), and `segtok` does
/// (`segtok-0.1.5/src/segmenter/mod.rs`'s `sentences()`:
/// `res.push(last.trim().to_string())`, read directly, not assumed), so
/// Verbora's sentences are trimmed *for the comparison only* — a documented
/// whitespace-attachment convention, not a boundary disagreement being
/// papered over. The untrimmed form is pinned separately by
/// [`sentence_tokenizer_does_not_trim`], so this normalization cannot hide a
/// regression in it. Returns the trimmed sentences so callers can add
/// ground-truth assertions.
fn assert_sentence_agreement(text: &str, label: &str) -> Vec<String> {
    let verbora_raw: Vec<&str> = SentenceTokenizer::new().tokens(text).collect();
    let verbora_sentences: Vec<String> = verbora_raw.iter().map(|s| s.trim().to_owned()).collect();

    // Exact, untrimmed: Verbora is the UAX #29 segmentation itself.
    assert_eq!(
        verbora_raw,
        text.split_sentence_bounds().collect::<Vec<_>>(),
        "verbora SentenceTokenizer must BE split_sentence_bounds() ({label})"
    );

    let unicode_sentences: Vec<String> = text
        .unicode_sentences()
        .map(|s| s.trim().to_owned())
        .collect();
    assert_eq!(
        verbora_sentences, unicode_sentences,
        "verbora SentenceTokenizer and unicode_sentences() (both trimmed) must agree ({label})"
    );

    let segtok_sentences = split_single(text, SegmentConfig::default());
    assert_eq!(
        verbora_sentences, segtok_sentences,
        "verbora SentenceTokenizer and segtok's split_single must agree ({label})"
    );

    verbora_sentences
}

/// The `sentence_tokenization` group on the fixed fixture.
#[test]
fn sentence_tokenization_agrees_on_narrowed_domain() {
    let text = narrowed_sentence_domain_text();
    let sentences = assert_sentence_agreement(&text, "fixed fixture");
    // Sanity: the fixture exercises enough sentences that a real boundary
    // mismatch could not hide inside it.
    assert!(sentences.len() > 30);
}

/// Lowercase forms that `segtok`'s `ABBREVIATIONS` regex
/// (`segtok-0.1.5/src/segmenter/abbreviations.rs`, read directly — not
/// assumed) can match at a candidate sentence end, in either the listed
/// lowercase form or the capitalized form a sentence-initial word takes.
/// The narrowed sentence domain has excluded abbreviations from day one;
/// this list makes that exclusion *enforceable by construction* when
/// sentence words are randomly generated instead of hand-picked — a random
/// word that happened to collide with `segtok`'s list would have left the
/// documented domain, not revealed a divergence inside it. Entries
/// unreachable by a `[a-z]{3,12}` generator (dotted forms like `e.g`,
/// non-ASCII like `jän`, one-to-two-letter forms like `cf`/`nr`/`st`) are
/// kept anyway so the list transcribes the regex faithfully rather than
/// editorially.
const SEGTOK_ABBREVIATION_WORDS: &[&str] = &[
    "approx", "abr", "apr", "aug", "capt", "cf", "col", "dec", "dez", "dic", "dr", "eg", "ene",
    "feb", "fe", "fig", "figs", "gen", "ie", "iv", "jan", "jul", "jun", "mag", "mar", "may", "med",
    "mr", "mrs", "mt", "nat", "no", "nov", "nr", "okt", "oct", "phil", "prof", "rer", "sci", "sep",
    "sept", "sgt", "spp", "sr", "sra", "srta", "st", "univ", "vol", "vs",
];

/// A random sentence-domain word: lowercase ASCII, length 3..=12 (the shared
/// corpus's own minimum length — `segtok` deliberately treats one- and
/// two-letter sentence-final words as initials/abbreviation-like, which the
/// domain's "no abbreviations" clause already excludes), rejecting the
/// [`SEGTOK_ABBREVIATION_WORDS`] collisions for the same documented reason.
fn random_sentence_word(rng: &mut SplitMix64) -> String {
    loop {
        let w = random_ascii_word(rng, 3, 12);
        if !SEGTOK_ABBREVIATION_WORDS.contains(&w.as_str()) {
            return w;
        }
    }
}

/// A random in-domain sentence document: `n_sentences` sentences of
/// `words_per_sentence` words each (first word capitalized, exactly one `.`
/// per sentence, exactly one joining space) — the same shape
/// `benches/tokenizers.rs`'s `sentence_prose` builds, but from seeded random
/// words instead of the shared corpus.
fn random_sentence_document(
    rng: &mut SplitMix64,
    n_sentences: usize,
    words_per_sentence: usize,
) -> String {
    let sentences: Vec<String> = (0..n_sentences)
        .map(|_| {
            let mut sentence = String::new();
            for w in 0..words_per_sentence {
                let word = random_sentence_word(rng);
                if w == 0 {
                    sentence.push(char::from(word.as_bytes()[0].to_ascii_uppercase()));
                    sentence.push_str(&word[1..]);
                } else {
                    sentence.push(' ');
                    sentence.push_str(&word);
                }
            }
            sentence.push('.');
            sentence
        })
        .collect();
    sentences.join(" ")
}

/// Seeded random sweep of the `sentence_tokenization` group: 40 documents
/// spanning 1..=40 sentences of 1..=12 words each. Two dimensions beyond
/// the fixed fixture's agreement: the sentence *count* must equal the
/// constructed count exactly (ground truth), and re-joining the trimmed
/// sentences with single spaces must reproduce the input byte-for-byte (no
/// boundary can silently eat or duplicate text).
#[test]
fn sentence_tokenization_agrees_on_seeded_random_documents() {
    let mut rng = SplitMix64(0x7031_C0DE_0006);

    for i in 0..40usize {
        // The two degenerate shapes first, then random sizes.
        let (n_sentences, words_per_sentence) = match i {
            0 => (1, 1),
            1 => (2, 1),
            _ => (1 + rng.below(40), 1 + rng.below(12)),
        };
        let text = random_sentence_document(&mut rng, n_sentences, words_per_sentence);
        let label = format!("{n_sentences} sentence(s) x {words_per_sentence} word(s)");

        let verbora_sentences = assert_sentence_agreement(&text, &label);
        assert_eq!(
            verbora_sentences.len(),
            n_sentences,
            "the constructed document has exactly {n_sentences} sentences ({label})"
        );
        assert_eq!(
            verbora_sentences.join(" "),
            text,
            "re-joining the trimmed sentences with single spaces must reproduce the \
             input byte-for-byte ({label})"
        );
    }
}

/// Fixed edge shapes of the sentence domain, including the exact
/// words-per-sentence densities (3, 6, 12, 24) the bench's
/// `sentence_tokenization_boundary_density` group times — this test is what
/// lets that group's doc comment claim agreement "including at these exact
/// densities" rather than extrapolating from the 6-word fixture.
#[test]
fn sentence_edge_shapes_agree_on_narrowed_domain() {
    let mut rng = SplitMix64(0x7031_C0DE_0007);

    // A single sentence; a single one-word sentence; a run of one-word
    // sentences; one very long sentence.
    for (n_sentences, words_per_sentence) in [(1, 8), (1, 1), (24, 1), (1, 120)] {
        let text = random_sentence_document(&mut rng, n_sentences, words_per_sentence);
        let label = format!("edge: {n_sentences} sentence(s) x {words_per_sentence} word(s)");
        let sentences = assert_sentence_agreement(&text, &label);
        assert_eq!(sentences.len(), n_sentences, "{label}");
    }

    // The boundary-density bench group's own shapes: a fixed 96-word budget
    // split into 3-, 6-, 12- and 24-word sentences.
    const DENSITY_TOTAL_WORDS: usize = 96;
    for words_per_sentence in [3, 6, 12, 24] {
        let n_sentences = DENSITY_TOTAL_WORDS / words_per_sentence;
        let text = random_sentence_document(&mut rng, n_sentences, words_per_sentence);
        let label = format!("density: {n_sentences} sentence(s) x {words_per_sentence} word(s)");
        let sentences = assert_sentence_agreement(&text, &label);
        assert_eq!(sentences.len(), n_sentences, "{label}");
        assert_eq!(sentences.join(" "), text, "{label}");
    }
}
