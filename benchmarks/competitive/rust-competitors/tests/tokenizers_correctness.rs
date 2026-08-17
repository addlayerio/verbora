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
