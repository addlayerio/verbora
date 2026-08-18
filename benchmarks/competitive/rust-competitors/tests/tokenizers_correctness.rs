//! `CORRECTNESS BEFORE PERFORMANCE` for `benches/tokenizers.rs`.
//!
//! `benches/tokenizers.rs`'s module doc comment narrows its input domain to
//! punctuation-free, single-ASCII-space-joined lowercase-letter text —
//! exactly the domain where Verbora's, tantivy's and Hugging Face
//! `tokenizers`' otherwise-divergent character classes and whitespace
//! definitions are documented to coincide. This file proves that claim: on
//! that domain, all three implementations in each of the two original
//! benchmarked groups (`whitespace_tokenization`, `word_tokenization`) draw
//! the exact same token boundaries — not merely the same token *count* —
//! before any timing number from that file is trusted.
//!
//! The remaining tests below do the same for this audit round's three added
//! groups (`word_tokenization_unicode_segmentation`,
//! `aggressive_tokenization_en`, `sentence_tokenization`) — see
//! `benches/tokenizers.rs`'s own module doc comment for exactly why each
//! narrowed domain is where the relevant implementations are expected to
//! agree, before proving it here.
//!
//! A later coverage-doubling round (the same one that widened
//! `benches/tokenizers.rs`'s size grids and added its
//! `sentence_tokenization_boundary_density` shape group) extends this file
//! along three axes, without touching any existing test or widening or
//! narrowing any documented exclusion:
//!
//! * **Seeded random sweeps** — deterministic `SplitMix64`-generated
//!   documents (dozens per group, word counts 1..=402, word lengths 1..=14)
//!   spanning far more of each narrowed domain than the fixed fixtures do,
//!   still generated *inside* those domains: lowercase ASCII `[a-z]` words,
//!   single U+0020 joins, and — for sentences — the same
//!   no-abbreviations clause the domain always had, enforced against
//!   `segtok`'s concrete abbreviation list (see
//!   [`SEGTOK_ABBREVIATION_WORDS`]).
//! * **Ground-truth agreement** — every sweep document is built from a known
//!   word (or sentence) sequence, so the tests assert each implementation
//!   reproduces that *constructed* sequence exactly, not merely that the
//!   implementations agree with each other (two implementations sharing a
//!   bug would pass a pure cross-check).
//! * **Byte-offset agreement** — the original tests compare token *text*;
//!   `word_boundary_byte_offsets_agree_on_narrowed_domain` additionally
//!   proves verbora, tantivy and Hugging Face place every boundary at the
//!   same byte *positions*, which token text alone cannot show (equal texts
//!   could in principle come from different positions in repetitive input).

use segtok::segmenter::{SegmentConfig, split_single};
use tantivy::tokenizer::{
    SimpleTokenizer, TokenStream as TantivyTokenStream, Tokenizer as TantivyTokenizer,
    WhitespaceTokenizer,
};
use tokenizers::pre_tokenizers::whitespace::{Whitespace, WhitespaceSplit};
use tokenizers::{OffsetReferential, OffsetType, PreTokenizedString, PreTokenizer};
use unicode_segmentation::UnicodeSegmentation;
use verbora_tokenizers::{
    AggressiveTokenizer, Pattern, RegexpTokenizer, SentenceTokenizer, Tokenize, WordTokenizer,
};

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
        .expect("Whitespace/WhitespaceSplit::pre_tokenize never fails");
    pretokenized
        .get_splits(OffsetReferential::Original, OffsetType::Byte)
        .into_iter()
        .map(|(s, _, _)| s.to_owned())
        .collect()
}

/// The `whitespace_tokenization` group: `verbora`'s `RegexpTokenizer(\s+)`,
/// `tantivy::WhitespaceTokenizer`, Hugging Face `WhitespaceSplit`.
#[test]
fn whitespace_tokenization_agrees_on_narrowed_domain() {
    let text = narrowed_domain_text();

    let verbora = RegexpTokenizer::new(Pattern::new(regex::Regex::new(r"\s+").unwrap()));
    let verbora_tokens: Vec<String> = verbora
        .tokenize(&text)
        .expect("`\\s+` has no way to fail to match punctuation-only text like this")
        .into_iter()
        .map(|t| {
            t.expect("`\\s+` has no capture groups, so no token is ever `undefined`")
                .to_owned()
        })
        .collect();

    let mut tantivy_tok = WhitespaceTokenizer::default();
    let tantivy_tokens = tantivy_tokens(&mut tantivy_tok, &text);

    let hf_tokens = hf_tokens(&WhitespaceSplit, &text);

    assert_eq!(
        verbora_tokens, tantivy_tokens,
        "verbora RegexpTokenizer(\\s+) and tantivy::WhitespaceTokenizer must draw identical \
         token boundaries on punctuation-free single-space-joined ASCII text"
    );
    assert_eq!(
        verbora_tokens, hf_tokens,
        "verbora RegexpTokenizer(\\s+) and Hugging Face WhitespaceSplit must draw identical \
         token boundaries on punctuation-free single-space-joined ASCII text"
    );
    // Sanity: the fixture exercises enough tokens that a real boundary
    // mismatch could not hide inside it.
    assert!(verbora_tokens.len() > 300);
}

/// The `word_tokenization` group: `verbora`'s `WordTokenizer`,
/// `tantivy::SimpleTokenizer`, Hugging Face `Whitespace`.
#[test]
fn word_tokenization_agrees_on_narrowed_domain() {
    let text = narrowed_domain_text();

    let verbora = WordTokenizer::new();
    let verbora_tokens: Vec<String> = verbora
        .tokenize(&text)
        .expect("WordTokenizer::tokenize never returns null")
        .into_iter()
        .map(str::to_owned)
        .collect();

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

/// The `word_tokenization_unicode_segmentation` group: `verbora`'s
/// `WordTokenizer` against `unicode_words()` (exact boundaries) and
/// `split_word_bounds()` (filtered to non-whitespace-only spans, since it
/// yields separators too — see `benches/tokenizers.rs`'s own doc comment).
#[test]
fn word_tokenization_agrees_with_unicode_segmentation_on_narrowed_domain() {
    let text = narrowed_domain_text();

    let verbora_tokens: Vec<String> = WordTokenizer::new()
        .tokenize(&text)
        .expect("WordTokenizer::tokenize never returns null")
        .into_iter()
        .map(str::to_owned)
        .collect();

    let unicode_words: Vec<String> = text.unicode_words().map(str::to_owned).collect();
    assert_eq!(
        verbora_tokens, unicode_words,
        "verbora WordTokenizer and unicode-segmentation's unicode_words() must draw \
         identical token boundaries on punctuation-free single-space-joined ASCII text \
         with no apostrophes, hyphens or digits"
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

/// The `aggressive_tokenization_en` group: `verbora`'s `AggressiveTokenizer`
/// (English) against `unicode_words()`. Only the English variant is in
/// scope — the matrix records `NO FAIR COMPETITOR FOUND` for the other 15
/// `AggressiveTokenizer` language variants (they deliberately reproduce
/// the reference bugs a Unicode-standard tokenizer cannot replicate).
#[test]
fn aggressive_tokenizer_en_agrees_with_unicode_words_on_narrowed_domain() {
    let text = narrowed_domain_text();

    let verbora_tokens: Vec<String> = AggressiveTokenizer::new()
        .tokenize(&text)
        .into_iter()
        .map(str::to_owned)
        .collect();
    let unicode_words: Vec<String> = text.unicode_words().map(str::to_owned).collect();

    assert_eq!(
        verbora_tokens, unicode_words,
        "verbora AggressiveTokenizer (English) and unicode-segmentation's unicode_words() \
         must draw identical token boundaries on punctuation-free single-space-joined ASCII \
         text with no apostrophes, hyphens or digits"
    );
    assert!(verbora_tokens.len() > 300);
}

/// A short document of plain declarative sentences: capitalized first word,
/// lowercase rest, exactly one `.` per sentence, exactly one space between
/// sentences, no digits/quotes/brackets/abbreviations/newlines — the
/// narrowed sentence-boundary domain `benches/tokenizers.rs`'s own
/// `sentence_prose` builds. Independent of `benches/data/words.json` so this
/// test needs no generated fixture, same convention as
/// `narrowed_domain_text` above.
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

/// The `sentence_tokenization` group: `verbora`'s `SentenceTokenizer`
/// against `unicode-segmentation`'s `unicode_sentences()`/
/// `split_sentence_bounds()` and `segtok`'s `split_single`.
///
/// `unicode-segmentation`'s two APIs keep the trailing delimiter-adjacent
/// whitespace attached to the preceding sentence by design (its own doc
/// example: `"Mr. Fox jumped..."` → `["Mr. ", "Fox jumped. ", ...]`) — a
/// documented formatting/whitespace-attachment difference, not a boundary
/// disagreement, so its spans are trimmed before comparison here (`verbora`
/// and `segtok` both already trim internally — confirmed by reading
/// `segtok-0.1.5/src/segmenter/mod.rs`'s `sentences()` directly, not
/// assumed).
#[test]
fn sentence_tokenization_agrees_on_narrowed_domain() {
    let text = narrowed_sentence_domain_text();

    let verbora_sentences: Vec<String> = SentenceTokenizer::new().tokenize(&text);

    let unicode_sentences: Vec<String> = text
        .unicode_sentences()
        .map(|s| s.trim().to_owned())
        .collect();
    assert_eq!(
        verbora_sentences, unicode_sentences,
        "verbora SentenceTokenizer and unicode-segmentation's unicode_sentences() (trimmed) \
         must agree on sentence boundaries for plain declarative sentences with no \
         abbreviations, digits, quotes or brackets"
    );

    let unicode_bounds: Vec<String> = text
        .split_sentence_bounds()
        .map(|s| s.trim().to_owned())
        .collect();
    assert_eq!(
        verbora_sentences, unicode_bounds,
        "verbora SentenceTokenizer and unicode-segmentation's split_sentence_bounds() \
         (trimmed) must agree on sentence boundaries on the same narrowed domain"
    );

    let segtok_sentences = split_single(&text, SegmentConfig::default());
    assert_eq!(
        verbora_sentences, segtok_sentences,
        "verbora SentenceTokenizer and segtok's split_single (already trimmed internally) \
         must agree on sentence boundaries on the same narrowed domain"
    );

    // Sanity: the fixture exercises enough sentences that a real boundary
    // mismatch could not hide inside it.
    assert!(verbora_sentences.len() > 30);
}

// ---------------------------------------------------------------------------
// Coverage-doubling round: seeded random sweeps, ground-truth agreement,
// byte-offset agreement, and edge shapes — see the module doc comment.
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
/// 1..=14, the shared corpus's own range extended down to the single-letter
/// words the existing fixture already contains), joined by exactly one
/// U+0020 space. Returns the document *and* the word sequence it was built
/// from, so callers can assert ground truth, not just cross-agreement.
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

/// `RegexpTokenizer(\s+)` tokens as owned strings (the
/// `whitespace_tokenization` group's verbora side).
fn verbora_whitespace_tokens(tok: &RegexpTokenizer, text: &str) -> Vec<String> {
    tok.tokenize(text)
        .expect("`\\s+` has no way to fail to match punctuation-only text like this")
        .into_iter()
        .map(|t| {
            t.expect("`\\s+` has no capture groups, so no token is ever `undefined`")
                .to_owned()
        })
        .collect()
}

/// `WordTokenizer` tokens as owned strings (the `word_tokenization` and
/// `word_tokenization_unicode_segmentation` groups' verbora side).
fn verbora_word_tokens(tok: &WordTokenizer, text: &str) -> Vec<String> {
    tok.tokenize(text)
        .expect("WordTokenizer::tokenize never returns null")
        .into_iter()
        .map(str::to_owned)
        .collect()
}

/// Seeded random sweep of the `whitespace_tokenization` group: 48 documents
/// spanning 1..=402 words. Beyond the three-way agreement the fixed fixture
/// already proves, every document's tokens are also checked against the
/// exact word sequence the document was constructed from — ground truth no
/// pure cross-check can provide.
#[test]
fn whitespace_tokenization_agrees_on_seeded_random_documents() {
    let mut rng = SplitMix64(0x7031_C0DE_0001);
    let verbora = RegexpTokenizer::new(Pattern::new(regex::Regex::new(r"\s+").unwrap()));
    let mut tantivy_tok = WhitespaceTokenizer::default();

    for n_words in sweep_word_counts(&mut rng, 48) {
        let (text, expected) = random_narrowed_document(&mut rng, n_words);

        let verbora_tokens = verbora_whitespace_tokens(&verbora, &text);
        assert_eq!(
            verbora_tokens, expected,
            "verbora RegexpTokenizer(\\s+) must reproduce the exact word sequence a \
             single-space-joined document was constructed from (n_words={n_words})"
        );
        assert_eq!(
            verbora_tokens,
            tantivy_tokens(&mut tantivy_tok, &text),
            "verbora and tantivy::WhitespaceTokenizer must agree on random in-domain \
             documents (n_words={n_words})"
        );
        assert_eq!(
            verbora_tokens,
            hf_tokens(&WhitespaceSplit, &text),
            "verbora and Hugging Face WhitespaceSplit must agree on random in-domain \
             documents (n_words={n_words})"
        );
    }
}

/// Seeded random sweep of the `word_tokenization` group — same sweep shape
/// and same ground-truth dimension as the whitespace sweep above, for
/// `WordTokenizer` / `tantivy::SimpleTokenizer` / Hugging Face `Whitespace`.
#[test]
fn word_tokenization_agrees_on_seeded_random_documents() {
    let mut rng = SplitMix64(0x7031_C0DE_0002);
    let verbora = WordTokenizer::new();
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

/// Seeded random sweep of the `word_tokenization_unicode_segmentation`
/// group: `WordTokenizer` against `unicode_words()` and the
/// whitespace-filtered `split_word_bounds()` (the same documented filter the
/// bench times), plus the ground-truth word sequence.
#[test]
fn unicode_segmentation_words_agree_on_seeded_random_documents() {
    let mut rng = SplitMix64(0x7031_C0DE_0003);
    let verbora = WordTokenizer::new();

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

/// Seeded random sweep of the `aggressive_tokenization_en` group. As
/// everywhere in this file, only the English `AggressiveTokenizer` is in
/// scope — the matrix's `NO FAIR COMPETITOR FOUND` verdict on the other 15
/// language variants is unchanged by this round.
#[test]
fn aggressive_tokenizer_en_agrees_on_seeded_random_documents() {
    let mut rng = SplitMix64(0x7031_C0DE_0004);
    let verbora = AggressiveTokenizer::new();

    for n_words in sweep_word_counts(&mut rng, 48) {
        let (text, expected) = random_narrowed_document(&mut rng, n_words);

        let verbora_tokens: Vec<String> = verbora
            .tokenize(&text)
            .into_iter()
            .map(str::to_owned)
            .collect();
        assert_eq!(
            verbora_tokens, expected,
            "verbora AggressiveTokenizer (English) must reproduce the constructed word \
             sequence (n_words={n_words})"
        );

        let unicode_words: Vec<String> = text.unicode_words().map(str::to_owned).collect();
        assert_eq!(
            verbora_tokens, unicode_words,
            "verbora AggressiveTokenizer (English) and unicode_words() must agree on \
             random in-domain documents (n_words={n_words})"
        );
    }
}

/// Byte span of `token` inside `text`, by pointer arithmetic — sound because
/// every verbora word-boundary tokenizer returns subslices borrowed from the
/// input (`Token<'a> = &'a str`), which the containment assert re-verifies.
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
        .expect("Whitespace/WhitespaceSplit::pre_tokenize never fails");
    pretokenized
        .get_splits(OffsetReferential::Original, OffsetType::Byte)
        .into_iter()
        .map(|(_, (start, end), _)| (start, end))
        .collect()
}

/// A new agreement dimension for both original word-boundary groups: the
/// implementations must place every token at the same byte *positions*, not
/// merely produce the same token *text*. On cyclic input like these
/// fixtures (and the bench's own cycled corpus), equal text sequences could
/// in principle be produced from different positions; offsets close that
/// gap. Verbora's offsets come from pointer arithmetic on its borrowed
/// `&str` tokens, tantivy's from `Token::offset_from`/`offset_to`, Hugging
/// Face's from `get_splits(.., OffsetType::Byte)`.
#[test]
fn word_boundary_byte_offsets_agree_on_narrowed_domain() {
    let mut rng = SplitMix64(0x7031_C0DE_0005);
    let fixed = narrowed_domain_text();
    let (random_a, _) = random_narrowed_document(&mut rng, 257);
    let (random_b, _) = random_narrowed_document(&mut rng, 31);

    let ws = RegexpTokenizer::new(Pattern::new(regex::Regex::new(r"\s+").unwrap()));
    let word = WordTokenizer::new();

    for text in [&fixed, &random_a, &random_b] {
        // `whitespace_tokenization` group.
        let verbora_ws_spans: Vec<(usize, usize)> = ws
            .tokenize(text)
            .expect("`\\s+` has no way to fail to match punctuation-only text like this")
            .into_iter()
            .map(|t| {
                span_in(
                    text,
                    t.expect("`\\s+` has no capture groups, so no token is ever `undefined`"),
                )
            })
            .collect();
        assert_eq!(
            verbora_ws_spans,
            tantivy_spans(&mut WhitespaceTokenizer::default(), text),
            "verbora RegexpTokenizer(\\s+) and tantivy::WhitespaceTokenizer must place \
             every token at identical byte offsets"
        );
        assert_eq!(
            verbora_ws_spans,
            hf_spans(&WhitespaceSplit, text),
            "verbora RegexpTokenizer(\\s+) and Hugging Face WhitespaceSplit must place \
             every token at identical byte offsets"
        );

        // `word_tokenization` group.
        let verbora_word_spans: Vec<(usize, usize)> = word
            .tokenize(text)
            .expect("WordTokenizer::tokenize never returns null")
            .into_iter()
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
        assert!(!verbora_ws_spans.is_empty());
        assert_eq!(verbora_ws_spans, verbora_word_spans);
    }
}

/// Fixed edge shapes of the word-boundary domain, checked across all five
/// word-boundary implementations at once (verbora, tantivy ×2, Hugging Face
/// ×2 pre-tokenizers, unicode-segmentation ×2) plus ground truth: a single
/// word (no separator anywhere), exactly one boundary, minimum-length (the
/// existing fixture's own `"a"`) and corpus-maximum (14-char) words,
/// alternating extremes, and one pathologically long single word — all still
/// lowercase ASCII words joined by single spaces, i.e. squarely inside the
/// documented narrowed domain.
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

    let ws = RegexpTokenizer::new(Pattern::new(regex::Regex::new(r"\s+").unwrap()));
    let word = WordTokenizer::new();
    let aggressive = AggressiveTokenizer::new();

    for expected in cases {
        let text = expected.join(" ");
        let label = format!("{} word(s), {} byte(s)", expected.len(), text.len());

        assert_eq!(
            verbora_whitespace_tokens(&ws, &text),
            expected,
            "verbora RegexpTokenizer(\\s+) must reproduce the constructed words ({label})"
        );
        assert_eq!(
            verbora_word_tokens(&word, &text),
            expected,
            "verbora WordTokenizer must reproduce the constructed words ({label})"
        );
        let aggressive_tokens: Vec<String> = aggressive
            .tokenize(&text)
            .into_iter()
            .map(str::to_owned)
            .collect();
        assert_eq!(
            aggressive_tokens, expected,
            "verbora AggressiveTokenizer (English) must reproduce the constructed words \
             ({label})"
        );

        assert_eq!(
            tantivy_tokens(&mut WhitespaceTokenizer::default(), &text),
            expected,
            "tantivy::WhitespaceTokenizer must agree on the edge shape ({label})"
        );
        assert_eq!(
            tantivy_tokens(&mut SimpleTokenizer::default(), &text),
            expected,
            "tantivy::SimpleTokenizer must agree on the edge shape ({label})"
        );
        assert_eq!(
            hf_tokens(&WhitespaceSplit, &text),
            expected,
            "Hugging Face WhitespaceSplit must agree on the edge shape ({label})"
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

/// Lowercase forms that `segtok`'s `ABBREVIATIONS` regex
/// (`segtok-0.1.5/src/segmenter/abbreviations.rs`, read directly — not
/// assumed) can match at a candidate sentence end, in either the listed
/// lowercase form or the capitalized form a sentence-initial word takes.
/// The narrowed sentence domain has excluded abbreviations from day one;
/// this list merely makes that same exclusion *enforceable by construction*
/// when sentence words are randomly generated instead of hand-picked — a
/// random word that happened to collide with `segtok`'s list would have
/// left the documented domain, not revealed a divergence inside it. Entries
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

/// Asserts the `sentence_tokenization` group's four implementations agree on
/// `text`, with `unicode-segmentation`'s spans trimmed for the same
/// documented whitespace-attachment reason as
/// `sentence_tokenization_agrees_on_narrowed_domain` (a formatting
/// convention, not a boundary disagreement — see that test's doc comment;
/// this round neither widens nor narrows that normalization). Returns
/// verbora's sentences so callers can add ground-truth assertions.
fn assert_sentence_four_way_agreement(text: &str, label: &str) -> Vec<String> {
    let verbora_sentences: Vec<String> = SentenceTokenizer::new().tokenize(text);

    let unicode_sentences: Vec<String> = text
        .unicode_sentences()
        .map(|s| s.trim().to_owned())
        .collect();
    assert_eq!(
        verbora_sentences, unicode_sentences,
        "verbora SentenceTokenizer and unicode_sentences() (trimmed) must agree ({label})"
    );

    let unicode_bounds: Vec<String> = text
        .split_sentence_bounds()
        .map(|s| s.trim().to_owned())
        .collect();
    assert_eq!(
        verbora_sentences, unicode_bounds,
        "verbora SentenceTokenizer and split_sentence_bounds() (trimmed) must agree ({label})"
    );

    let segtok_sentences = split_single(text, SegmentConfig::default());
    assert_eq!(
        verbora_sentences, segtok_sentences,
        "verbora SentenceTokenizer and segtok's split_single must agree ({label})"
    );

    verbora_sentences
}

/// Seeded random sweep of the `sentence_tokenization` group: 40 documents
/// spanning 1..=40 sentences of 1..=12 words each. Two dimensions beyond
/// the fixed fixture's four-way agreement: the sentence *count* must equal
/// the constructed count exactly (ground truth), and re-joining verbora's
/// trimmed sentences with single spaces must reproduce the input
/// byte-for-byte (no boundary can silently eat or duplicate text).
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

        let verbora_sentences = assert_sentence_four_way_agreement(&text, &label);
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
        let sentences = assert_sentence_four_way_agreement(&text, &label);
        assert_eq!(sentences.len(), n_sentences, "{label}");
    }

    // The boundary-density bench group's own shapes: a fixed 96-word budget
    // split into 3-, 6-, 12- and 24-word sentences.
    const DENSITY_TOTAL_WORDS: usize = 96;
    for words_per_sentence in [3, 6, 12, 24] {
        let n_sentences = DENSITY_TOTAL_WORDS / words_per_sentence;
        let text = random_sentence_document(&mut rng, n_sentences, words_per_sentence);
        let label = format!("density: {n_sentences} sentence(s) x {words_per_sentence} word(s)");
        let sentences = assert_sentence_four_way_agreement(&text, &label);
        assert_eq!(sentences.len(), n_sentences, "{label}");
        assert_eq!(sentences.join(" "), text, "{label}");
    }
}
