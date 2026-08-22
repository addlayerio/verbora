//! The five ways to produce tags must produce the same tags.
//!
//! `BrillTagger`'s "Choosing the right API" table claims that `tag`, `tag_into`,
//! `tag_stream`, `annotate` + `transform` and `par_tag_batch` differ only in
//! where the memory goes. These tests are that claim, run over the crate's own
//! awkward-input sweep, over inputs long enough to cross `tag_stream`'s block
//! boundary, and under a rule set whose context is wide enough that a streaming
//! implementation cannot get away with buffering too little.

use verbora_tagger::{BrillTagger, Lexicon, RuleSet, Tag, TaggedToken};

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

fn tag(s: &'static str) -> Tag {
    Tag::new(s).expect("a conforming tag")
}

fn corpus_of(len: usize) -> Vec<&'static str> {
    AWKWARD.iter().copied().cycle().take(len).collect()
}

/// A narrow configuration: a handful of entries and rules that each read one or
/// two tokens of context, which is the ordinary case.
fn narrow() -> (Lexicon, RuleSet) {
    let mut lexicon = Lexicon::new(tag("NN")).with_capitalized_default_tag(tag("NNP"));
    for (key, tags) in [
        (".", vec!["."]),
        ("a", vec!["DT"]),
        ("café", vec!["NN"]),
        ("don't", vec!["VBP"]),
        ("well-known", vec!["JJ"]),
        ("z", vec!["NN", "SYM"]),
    ] {
        lexicon
            .insert(key, tags.into_iter().map(tag).collect())
            .expect("a conforming entry");
    }
    let rules: RuleSet = "NN CD CURRENT-WORD-IS-NUMBER YES\n\
         NN URL CURRENT-WORD-IS-URL YES\n\
         NNP URL CURRENT-WORD-IS-URL YES\n\
         NN NNS CURRENT-WORD-ENDS-WITH s\n\
         NNP NNPS CURRENT-WORD-ENDS-WITH s\n\
         NN JJ NEXT-TAG NN\n\
         DT WDT PREV-1-OR-2-TAG NN\n\
         NNP NN PREV-TAG DT"
        .parse()
        .expect("well-formed rules");
    (lexicon, rules)
}

/// A wide configuration: twenty-four rules that chain, so a single `seed` token
/// changes tags twelve positions away in each direction.
///
/// `context_span` is therefore `(12, 12)` and every one of those tokens is
/// genuinely load-bearing — a streaming run that buffered less would disagree
/// with a whole-document run rather than merely risking it.
fn wide() -> (Lexicon, RuleSet) {
    use std::fmt::Write as _;
    const N: usize = 12;

    let mut lexicon = Lexicon::new(tag("A"));
    lexicon
        .insert("seed", vec![tag("S")])
        .expect("a conforming entry");
    lexicon
        .insert("Zzzzz", vec![tag("S")])
        .expect("a conforming entry");
    let mut text = String::new();
    for k in 0..N {
        let from = if k == 0 {
            "S".to_owned()
        } else {
            format!("T{k}")
        };
        writeln!(text, "A T{} PREV-TAG {from}", k + 1).expect("writing to a String");
        let from = if k == 0 {
            "S".to_owned()
        } else {
            format!("U{k}")
        };
        writeln!(text, "A U{} NEXT-TAG {from}", k + 1).expect("writing to a String");
    }
    let rules: RuleSet = text.parse().expect("well-formed rules");
    assert_eq!(rules.context_span(), (N, N));
    (lexicon, rules)
}

fn check(what: &str, lexicon: &Lexicon, rules: &RuleSet, tokens: &[&str]) {
    let tagger = BrillTagger::new(lexicon, rules);

    let batch = tagger.tag(tokens.iter().copied());
    assert_eq!(batch.len(), tokens.len());

    let mut into = Vec::new();
    tagger.tag_into(tokens.iter().copied(), &mut into);
    assert_eq!(
        into,
        batch,
        "{what}: tag_into differs at {} tokens",
        tokens.len()
    );

    let streamed: Vec<TaggedToken<'_>> = tagger.tag_stream(tokens.iter().copied()).collect();
    assert_eq!(
        streamed,
        batch,
        "{what}: tag_stream differs at {} tokens",
        tokens.len()
    );

    let mut staged = tagger.annotate(tokens.iter().copied());
    tagger.transform(&mut staged);
    assert_eq!(
        staged,
        batch,
        "{what}: annotate+transform differs at {} tokens",
        tokens.len()
    );

    // Tokens are never rewritten, whichever route produced them.
    for (got, want) in batch.iter().zip(tokens) {
        assert_eq!(got.token(), *want);
    }
}

const LENGTHS: [usize; 9] = [0, 1, 2, 27, 100, 1023, 1024, 1025, 2049];

#[test]
fn the_apis_agree_across_the_block_boundary() {
    let (lexicon, rules) = narrow();
    for n in LENGTHS {
        check("narrow", &lexicon, &rules, &corpus_of(n));
    }
}

#[test]
fn the_apis_agree_under_a_wide_context() {
    let (lexicon, rules) = wide();
    for n in LENGTHS {
        check("wide", &lexicon, &rules, &corpus_of(n));
    }
    // The awkward sweep seeds every 27 tokens; a second pass seeds at a period
    // coprime with the 1024-token block, so a chain straddles a boundary.
    let seeded: Vec<&str> = (0..2500)
        .map(|i| if i % 37 == 0 { "seed" } else { "x" })
        .collect();
    for n in LENGTHS.into_iter().chain([2100, 2500]) {
        check("wide/seeded", &lexicon, &rules, &seeded[..n]);
    }
}

/// The wide configuration really does propagate the full distance, or the
/// equivalence above would be passing on a rule set that never used its context.
#[test]
fn the_wide_rule_set_reaches_the_end_of_its_context() {
    let (lexicon, rules) = wide();
    let tagger = BrillTagger::new(&lexicon, &rules);
    let tokens: Vec<&str> = std::iter::once("seed")
        .chain(std::iter::repeat_n("x", 20))
        .collect();
    let tagged = tagger.tag(tokens.iter().copied());
    let tags: Vec<&str> = tagged.iter().map(|w| w.tag().as_str()).collect();
    assert_eq!(tags[0], "S");
    assert_eq!(tags[12], "T12", "the chain travelled twelve tokens");
    assert_eq!(tags[13], "A", "and no further");
}

#[cfg(feature = "parallel")]
#[test]
fn par_tag_batch_agrees_with_the_sequential_loop() {
    for (what, (lexicon, rules)) in [("narrow", narrow()), ("wide", wide())] {
        let tagger = BrillTagger::new(&lexicon, &rules);
        let documents: Vec<Vec<&str>> = [0usize, 1, 27, 1025].into_iter().map(corpus_of).collect();
        let sequential: Vec<_> = documents
            .iter()
            .map(|d| tagger.tag(d.iter().copied()))
            .collect();
        assert_eq!(tagger.par_tag_batch(&documents), sequential, "{what}");
    }
}
