//! The five ways to produce tags must produce the same tags.
//!
//! `BrillTagger`'s "Choosing the right API" table claims that `tag`, `tag_into`,
//! `tag_stream`, `annotate` + `transform` and `par_tag_batch` differ only in
//! where the memory goes. These tests are that claim, run over the crate's own
//! awkward-input sweep and over inputs long enough to cross `tag_stream`'s block
//! boundary in both bundled languages.

use verbora_tagger::{BrillTagger, Language, Lexicon, RuleSet, TaggedToken};

/// One token per script and character class the crate is expected to survive.
const AWKWARD: &[&str] = &[
    "",
    "z",
    "Z",
    "Zzzzz",
    "café",
    "Café",
    "Ålesundzz",
    "Москвазз",
    "Ελλάςζζ",
    "ΟΔΟΣΣΣ",
    "İstanbulzz",
    "straßezz",
    "日本語",
    "😀",
    "𝐀bc",
    ".",
    "5",
    "3.14",
    "1,000",
    "www.example.com",
    "well-known",
    "A.A.U.",
    "%CHG",
    "a b",
    "\u{feff}",
    "don't",
    "node_js",
];

fn corpus_of(len: usize) -> Vec<&'static str> {
    AWKWARD.iter().copied().cycle().take(len).collect()
}

fn check(language: Language, tokens: &[&str]) {
    let lexicon = Lexicon::bundled(language);
    let rules = RuleSet::bundled(language);
    let tagger = BrillTagger::new(&lexicon, &rules);

    let batch = tagger.tag(tokens.iter().copied());
    assert_eq!(batch.len(), tokens.len());

    let mut into = Vec::new();
    tagger.tag_into(tokens.iter().copied(), &mut into);
    assert_eq!(
        into,
        batch,
        "{language:?}: tag_into differs at {} tokens",
        tokens.len()
    );

    let streamed: Vec<TaggedToken<'_>> = tagger.tag_stream(tokens.iter().copied()).collect();
    assert_eq!(
        streamed,
        batch,
        "{language:?}: tag_stream differs at {} tokens",
        tokens.len()
    );

    let mut staged = tagger.annotate(tokens.iter().copied());
    tagger.transform(&mut staged);
    assert_eq!(
        staged,
        batch,
        "{language:?}: annotate+transform differs at {} tokens",
        tokens.len()
    );

    // Tokens are never rewritten, whichever route produced them.
    for (got, want) in batch.iter().zip(tokens) {
        assert_eq!(got.token(), *want);
    }
}

#[test]
fn english_apis_agree_across_the_block_boundary() {
    for n in [0, 1, 2, 27, 100, 1023, 1024, 1025, 2049] {
        check(Language::English, &corpus_of(n));
    }
}

#[test]
fn dutch_apis_agree_across_the_block_boundary() {
    // 285 rules, so `context_span` is wide and the streaming margin matters.
    for n in [0, 1, 2, 27, 100, 1023, 1024, 1025, 2049] {
        check(Language::Dutch, &corpus_of(n));
    }
}

#[cfg(feature = "parallel")]
#[test]
fn par_tag_batch_agrees_with_the_sequential_loop() {
    let lexicon = Lexicon::bundled(Language::English);
    let rules = RuleSet::bundled(Language::English);
    let tagger = BrillTagger::new(&lexicon, &rules);

    let documents: Vec<Vec<&str>> = [0usize, 1, 27, 1025].into_iter().map(corpus_of).collect();
    let sequential: Vec<_> = documents
        .iter()
        .map(|d| tagger.tag(d.iter().copied()))
        .collect();
    assert_eq!(tagger.par_tag_batch(&documents), sequential);
}
