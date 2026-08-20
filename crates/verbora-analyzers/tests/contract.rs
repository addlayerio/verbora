//! The clause-analysis contract, pinned end to end.
//!
//! Every expected value here is derived from the rules stated in the crate
//! documentation and from the published sources those rules cite — the Penn
//! Treebank tag set for the tags, and standard descriptive grammar for the
//! clause types. None was produced by running this crate and recording what
//! came out.

use verbora_analyzers::{
    ImpliedSubject, Role, SentenceAnalysis, SentenceType, TagClass, TaggedWord as W, Terminator,
    analyze, analyze_with_terminator,
};

/// The 36 word tags of the Penn Treebank tag set, transcribed from Santorini's
/// guidelines, plus the punctuation tags the treebank uses.
const PENN_TAGS: [&str; 45] = [
    "CC", "CD", "DT", "EX", "FW", "IN", "JJ", "JJR", "JJS", "LS", "MD", "NN", "NNS", "NNP", "NNPS",
    "PDT", "POS", "PRP", "PRP$", "RB", "RBR", "RBS", "RP", "SYM", "TO", "UH", "VB", "VBD", "VBG",
    "VBN", "VBP", "VBZ", "WDT", "WP", "WP$", "WRB", ".", ",", ":", "``", "''", "-LRB-", "-RRB-",
    "#", "$",
];

fn tokens<'a>(words: impl Iterator<Item = &'a str>) -> Vec<&'a str> {
    words.collect()
}

/// The prepositional phrases as `(start, end)` pairs, which compare more
/// legibly than `Range` literals.
fn phrases(analysis: &SentenceAnalysis<'_>) -> Vec<(usize, usize)> {
    analysis
        .prepositional_phrases()
        .iter()
        .map(|range| (range.start, range.end))
        .collect()
}

// ---------------------------------------------------------------------------
// Stage 3 — the subject/predicate split
// ---------------------------------------------------------------------------

/// Every Penn verb tag opens the predicate, not only the base form `VB`.
///
/// *The bear <verb> the squirrel.* — whichever verb tag the tagger assigned,
/// the subject is *The bear*. Enumerated over all seven verb tags rather than
/// sampled: the previous implementation matched only `VB`, and a test using
/// `VB` alone would have passed against it.
#[test]
fn all_seven_verb_tags_open_the_predicate() {
    for tag in ["MD", "VB", "VBD", "VBG", "VBN", "VBP", "VBZ"] {
        let sentence = [
            W::new("The", "DT"),
            W::new("bear", "NN"),
            W::new("chased", tag),
            W::new("it", "PRP"),
            W::new(".", "."),
        ];
        let analysis = analyze(&sentence);
        assert_eq!(
            tokens(analysis.subject_tokens()),
            ["The", "bear"],
            "subject with verb tag {tag}"
        );
        assert_eq!(
            tokens(analysis.predicate_tokens()),
            ["chased", "it"],
            "predicate with verb tag {tag}"
        );
        assert_eq!(analysis.role(4), Some(Role::Terminator), "{tag}");
    }
}

/// No non-verb tag opens the predicate. Enumerated over the whole tag set, so
/// the complement of the verb family is checked rather than assumed.
#[test]
fn no_non_verb_tag_opens_the_predicate() {
    let mut verbs = 0;
    let mut non_verbs = 0;
    for tag in PENN_TAGS {
        let sentence = [W::new("a", "DT"), W::new("x", tag), W::new("b", "DT")];
        let analysis = analyze(&sentence);
        if TagClass::of(tag).is_verb() {
            verbs += 1;
            assert_eq!(analysis.role(1), Some(Role::Predicate), "{tag}");
        } else {
            non_verbs += 1;
            // `IN` is the one non-verb tag that changes a role, and it changes
            // it to a phrase rather than to the predicate.
            let expected = if TagClass::of(tag) == TagClass::Preposition {
                Role::PrepositionalPhrase
            } else {
                Role::Subject
            };
            assert_eq!(analysis.role(1), Some(expected), "{tag}");
        }
    }
    assert_eq!(verbs, 7);
    assert_eq!(non_verbs, 38);
    assert_eq!(verbs + non_verbs, PENN_TAGS.len());
}

/// Existential *there* is the subject, and the verb after it opens the
/// predicate like any other. *There is a house in the valley.*
#[test]
fn existential_there_is_the_subject() {
    let sentence = [
        W::new("There", "EX"),
        W::new("is", "VBZ"),
        W::new("a", "DT"),
        W::new("house", "NN"),
        W::new("in", "IN"),
        W::new("the", "DT"),
        W::new("valley", "NN"),
        W::new(".", "."),
    ];
    let analysis = analyze(&sentence);
    assert_eq!(analysis.subject_to_string(), "There");
    assert_eq!(analysis.predicate_to_string(), "is a house");
    assert_eq!(phrases(&analysis), [(4, 7)]);
    assert_eq!(analysis.sentence_type(), Some(SentenceType::Declarative));
}

/// A verb inside a prepositional phrase does not open the predicate.
/// *The bear in the running water drank.* — the predicate starts at *drank*.
#[test]
fn a_verb_inside_a_phrase_does_not_open_the_predicate() {
    let sentence = [
        W::new("The", "DT"),
        W::new("bear", "NN"),
        W::new("in", "IN"),
        W::new("the", "DT"),
        W::new("running", "VBG"),
        W::new("water", "NN"),
        W::new("drank", "VBD"),
        W::new(".", "."),
    ];
    let analysis = analyze(&sentence);
    assert_eq!(analysis.subject_to_string(), "The bear");
    assert_eq!(analysis.predicate_to_string(), "drank");
    assert_eq!(phrases(&analysis), [(2, 6)]);
    assert_eq!(
        analysis.roles(),
        [
            Role::Subject,
            Role::Subject,
            Role::PrepositionalPhrase,
            Role::PrepositionalPhrase,
            Role::PrepositionalPhrase,
            Role::PrepositionalPhrase,
            Role::Predicate,
            Role::Terminator,
        ]
    );
}

/// With no verb anywhere, the whole body is subject and the predicate is empty.
#[test]
fn a_verbless_body_is_all_subject() {
    let sentence = [W::new("The", "DT"), W::new("bear", "NN")];
    let analysis = analyze(&sentence);
    assert_eq!(analysis.subject_to_string(), "The bear");
    assert_eq!(analysis.predicate_to_string(), "");
    assert_eq!(analysis.sentence_type(), None);
}

// ---------------------------------------------------------------------------
// Stage 1 — prepositional phrases
// ---------------------------------------------------------------------------

/// A phrase closes at the first following noun, inclusive, and each of the four
/// noun tags closes it.
#[test]
fn every_noun_tag_closes_a_phrase_inclusively() {
    for tag in ["NN", "NNS", "NNP", "NNPS"] {
        let sentence = [
            W::new("cat", "NN"),
            W::new("in", "IN"),
            W::new("the", "DT"),
            W::new("hats", tag),
            W::new("today", "RB"),
        ];
        let analysis = analyze(&sentence);
        assert_eq!(phrases(&analysis), [(1, 4)], "{tag}");
        assert_eq!(analysis.role(3), Some(Role::PrepositionalPhrase), "{tag}");
        assert_eq!(analysis.role(4), Some(Role::Subject), "{tag}");
    }
}

/// Adjacent phrases: *in the house on the hill* is two phrases, not one.
#[test]
fn adjacent_phrases_are_separate() {
    let sentence = [
        W::new("in", "IN"),
        W::new("the", "DT"),
        W::new("house", "NN"),
        W::new("on", "IN"),
        W::new("the", "DT"),
        W::new("hill", "NN"),
    ];
    let analysis = analyze(&sentence);
    assert_eq!(phrases(&analysis), [(0, 3), (3, 6)]);
    assert!(
        analysis
            .roles()
            .iter()
            .all(|r| *r == Role::PrepositionalPhrase)
    );
}

/// A second `IN` while a phrase is open does not open a new one.
#[test]
fn a_preposition_inside_an_open_phrase_is_absorbed() {
    let sentence = [
        W::new("in", "IN"),
        W::new("in", "IN"),
        W::new("the", "DT"),
        W::new("house", "NN"),
    ];
    assert_eq!(phrases(&analyze(&sentence)), [(0, 4)]);
}

/// A phrase with no following noun runs to the end of the body — and stops at
/// the body, never swallowing the terminator.
#[test]
fn an_unterminated_phrase_stops_at_the_terminator() {
    let sentence = [
        W::new("Vote", "VB"),
        W::new("for", "IN"),
        W::new("me", "PRP"),
        W::new("!", "."),
    ];
    let analysis = analyze(&sentence);
    assert_eq!(phrases(&analysis), [(1, 3)]);
    assert_eq!(analysis.role(3), Some(Role::Terminator));
}

/// `TO` is not a preposition here, because Penn Treebank conflates the
/// infinitive marker with the preposition.
#[test]
fn to_does_not_open_a_phrase() {
    let sentence = [W::new("go", "VB"), W::new("to", "TO"), W::new("bed", "NN")];
    assert!(analyze(&sentence).prepositional_phrases().is_empty());
}

// ---------------------------------------------------------------------------
// Stage 2 and 4 — imperatives and clause type
// ---------------------------------------------------------------------------

/// An imperative has no overt subject and reports the understood *you*.
#[test]
fn an_imperative_reports_an_implied_subject_without_inserting_a_word() {
    let sentence = [W::new("Vote", "VB"), W::new("now", "RB"), W::new(".", ".")];
    let analysis = analyze(&sentence);

    assert_eq!(
        analysis.implied_subject(),
        Some(ImpliedSubject::SecondPerson)
    );
    assert_eq!(
        analysis.implied_subject().map(ImpliedSubject::pronoun),
        Some("you")
    );
    assert_eq!(analysis.sentence_type(), Some(SentenceType::Imperative));
    assert!(tokens(analysis.subject_tokens()).is_empty());
    assert_eq!(tokens(analysis.predicate_tokens()), ["Vote", "now"]);

    // No synthetic word was added: the analysis has exactly one role per input
    // word, and the caller's slice still holds three words.
    assert_eq!(analysis.roles().len(), sentence.len());
    assert_eq!(analysis.sentence().len(), 3);
}

/// Only `VB` heads an imperative; a finite verb in first position is inversion,
/// which reads as a question instead. Enumerated over every verb tag.
#[test]
fn only_the_base_form_heads_an_imperative() {
    for tag in ["MD", "VB", "VBD", "VBG", "VBN", "VBP", "VBZ"] {
        let sentence = [W::new("x", tag), W::new("it", "PRP")];
        let analysis = analyze(&sentence);
        let imperative = analysis.implied_subject().is_some();
        assert_eq!(imperative, tag == "VB", "{tag}");

        let expected = match tag {
            "VB" => Some(SentenceType::Imperative),
            // `MD`, `VBD`, `VBP`, `VBZ` are finite: initial position is
            // inversion, and the trailing pronoun is also a tag question.
            "MD" | "VBD" | "VBP" | "VBZ" => Some(SentenceType::Interrogative),
            // `VBG`/`VBN` are participial adjuncts and carry no cue.
            _ => None,
        };
        assert_eq!(analysis.sentence_type(), expected, "{tag}");
    }
}

/// Adverbs and interjections may precede the imperative verb; nothing else may.
#[test]
fn only_adverbs_and_interjections_precede_an_imperative_verb() {
    let mut skippable = 0;
    for tag in PENN_TAGS {
        let sentence = [W::new("x", tag), W::new("look", "VB")];
        let analysis = analyze(&sentence);
        let skips = matches!(TagClass::of(tag), TagClass::Adverb | TagClass::Interjection);
        if skips {
            skippable += 1;
        }
        // The clause is imperative when the first word is skipped and `look`
        // becomes the head, or when the first word is itself a base verb.
        let expected = skips || tag == "VB";
        assert_eq!(
            analysis.implied_subject().is_some(),
            expected,
            "{tag} before a base verb"
        );
    }
    // RB, RBR, RBS and UH.
    assert_eq!(skippable, 4);
}

/// The documented limit: a comma between the interjection and the verb defeats
/// the imperative test, because a comma's tag is neither adverb nor
/// interjection.
#[test]
fn a_comma_before_the_verb_defeats_the_imperative_test() {
    let with_comma = [
        W::new("Please", "UH"),
        W::new(",", ","),
        W::new("vote", "VB"),
    ];
    assert_eq!(analyze(&with_comma).implied_subject(), None);

    let without = [W::new("Please", "UH"), W::new("vote", "VB")];
    assert_eq!(
        analyze(&without).implied_subject(),
        Some(ImpliedSubject::SecondPerson)
    );
}

/// The full terminator/clause matrix from the crate documentation, walked in
/// both clause shapes.
#[test]
fn the_terminator_matrix_holds_in_both_clause_shapes() {
    let imperative = [W::new("Vote", "VB"), W::new("now", "RB")];
    let declarative = [W::new("It", "PRP"), W::new("rained", "VBD")];

    let cases = [
        (
            Terminator::Question,
            SentenceType::Interrogative,
            SentenceType::Interrogative,
        ),
        (
            Terminator::Exclamation,
            SentenceType::Imperative,
            SentenceType::Exclamative,
        ),
        (
            Terminator::FullStop,
            SentenceType::Imperative,
            SentenceType::Declarative,
        ),
    ];

    for (terminator, when_imperative, otherwise) in cases {
        // Every scalar of the kind must give the same answer, not just the
        // ASCII one.
        for &scalar in terminator.scalars() {
            let mark = scalar.to_string();

            let mut sentence = imperative.to_vec();
            sentence.push(W::new(mark.clone(), "."));
            assert_eq!(
                analyze(&sentence).sentence_type(),
                Some(when_imperative),
                "{terminator} {scalar:?} on an imperative"
            );

            let mut sentence = declarative.to_vec();
            sentence.push(W::new(mark, "."));
            assert_eq!(
                analyze(&sentence).sentence_type(),
                Some(otherwise),
                "{terminator} {scalar:?} on a declarative"
            );
        }
    }
}

/// Without a terminator, wh-initial position reads as a question — for each of
/// the four wh-tags, and for none of the others.
#[test]
fn wh_initial_position_reads_as_a_question() {
    for tag in ["WDT", "WP", "WP$", "WRB"] {
        let sentence = [W::new("Who", tag), W::new("voted", "VBD")];
        assert_eq!(
            analyze(&sentence).sentence_type(),
            Some(SentenceType::Interrogative),
            "{tag}"
        );
    }
    // Not first: no cue, so no classification.
    let later = [W::new("a", "NN"), W::new("who", "WP"), W::new("z", "JJ")];
    assert_eq!(analyze(&later).sentence_type(), None);
}

/// A tag question: an operator plus a personal pronoun, with adverbs allowed
/// between them (Quirk et al. §11.8).
#[test]
fn a_trailing_tag_question_reads_as_a_question() {
    let plain = [W::new("Should", "MD"), W::new("we", "PRP")];
    assert_eq!(
        analyze(&plain).sentence_type(),
        Some(SentenceType::Interrogative)
    );

    let negated = [
        W::new("It", "PRP"),
        W::new("is", "VBZ"),
        W::new("cold", "JJ"),
        W::new(",", ","),
        W::new("is", "VBZ"),
        W::new("n't", "RB"),
        W::new("it", "PRP"),
    ];
    assert_eq!(
        analyze(&negated).sentence_type(),
        Some(SentenceType::Interrogative)
    );

    // A pronoun with no operator before it is not a tag question.
    let bare = [W::new("cold", "JJ"), W::new("it", "PRP")];
    assert_eq!(analyze(&bare).sentence_type(), None);

    // A lone pronoun has nothing before it at all.
    let lone = [W::new("it", "PRP")];
    assert_eq!(analyze(&lone).sentence_type(), None);
}

/// Punctuation wins over the clause cue when both are present, as documented.
#[test]
fn a_terminator_overrides_the_clause_cue() {
    let with_stop = [
        W::new("What", "WP"),
        W::new("he", "PRP"),
        W::new("said", "VBD"),
        W::new(".", "."),
    ];
    assert_eq!(
        analyze(&with_stop).sentence_type(),
        Some(SentenceType::Declarative)
    );

    let without = [
        W::new("What", "WP"),
        W::new("he", "PRP"),
        W::new("said", "VBD"),
    ];
    assert_eq!(
        analyze(&without).sentence_type(),
        Some(SentenceType::Interrogative)
    );
}

// ---------------------------------------------------------------------------
// Stage 0 — terminators, and the two entry points
// ---------------------------------------------------------------------------

/// The terminator is decided by the token, not by the tag: a mark tagged
/// anything at all still ends the sentence, and a word tagged `.` that is not a
/// recognised mark does not.
#[test]
fn the_terminator_is_decided_by_the_token() {
    for tag in PENN_TAGS {
        let sentence = [
            W::new("It", "PRP"),
            W::new("rained", "VBD"),
            W::new("?", tag),
        ];
        let analysis = analyze(&sentence);
        assert_eq!(analysis.terminator(), Some(Terminator::Question), "{tag}");
        assert_eq!(analysis.terminator_index(), Some(2), "{tag}");
    }

    let not_a_mark = [
        W::new("It", "PRP"),
        W::new("rained", "VBD"),
        W::new(";", "."),
    ];
    let analysis = analyze(&not_a_mark);
    assert_eq!(analysis.terminator(), None);
    assert_eq!(analysis.terminator_index(), None);
    assert_eq!(analysis.role(2), Some(Role::Predicate));
}

/// Only the *last* word can be the terminator; an interior mark is an ordinary
/// word.
#[test]
fn only_the_last_word_can_be_the_terminator() {
    let sentence = [
        W::new("It", "PRP"),
        W::new(".", "."),
        W::new("rained", "VBD"),
    ];
    let analysis = analyze(&sentence);
    assert_eq!(analysis.terminator(), None);
    assert_eq!(analysis.role(1), Some(Role::Subject));
}

/// The out-of-band form never consumes a word and never reports an index.
#[test]
fn analyze_with_terminator_consumes_no_word() {
    let stripped = [W::new("Who", "WP"), W::new("voted", "VBD")];
    let analysis = analyze_with_terminator(&stripped, Some(Terminator::Question));
    assert_eq!(analysis.terminator(), Some(Terminator::Question));
    assert_eq!(analysis.terminator_index(), None);
    assert_eq!(analysis.roles().len(), 2);
    assert!(!analysis.roles().contains(&Role::Terminator));
    assert_eq!(analysis.sentence_type(), Some(SentenceType::Interrogative));

    // `None` keeps a trailing mark as an ordinary word.
    let kept = [
        W::new("It", "PRP"),
        W::new("rained", "VBD"),
        W::new(".", "."),
    ];
    let analysis = analyze_with_terminator(&kept, None);
    assert_eq!(analysis.terminator(), None);
    assert_eq!(analysis.role(2), Some(Role::Predicate));
    assert_eq!(analysis.sentence_type(), None);
}

// ---------------------------------------------------------------------------
// Totality, purity and edge cases
// ---------------------------------------------------------------------------

/// The empty sentence: no panic, nothing to report, no implied subject.
#[test]
fn the_empty_sentence_analyses_to_nothing() {
    let analysis = analyze(&[]);
    assert!(analysis.roles().is_empty());
    assert!(analysis.prepositional_phrases().is_empty());
    assert_eq!(analysis.terminator(), None);
    assert_eq!(analysis.terminator_index(), None);
    assert_eq!(analysis.implied_subject(), None);
    assert_eq!(analysis.sentence_type(), None);
    assert_eq!(analysis.subject_to_string(), "");
    assert_eq!(analysis.predicate_to_string(), "");
    assert_eq!(analysis.role(0), None);

    // A sentence that is nothing but a terminator has an empty body.
    let only_mark = [W::new(".", ".")];
    let analysis = analyze(&only_mark);
    assert_eq!(analysis.roles(), [Role::Terminator]);
    assert_eq!(analysis.sentence_type(), Some(SentenceType::Declarative));
    assert_eq!(analysis.subject_to_string(), "");
}

/// Every word gets exactly one role, and the roles partition the sentence.
/// Walked over every Penn tag in every position of a three-word sentence.
#[test]
fn roles_partition_every_sentence() {
    for tag in PENN_TAGS {
        for position in 0..3 {
            let mut sentence = [W::new("a", "DT"), W::new("b", "VBD"), W::new("c", "NN")];
            sentence[position] = W::new("x", tag);
            let analysis = analyze(&sentence);

            assert_eq!(
                analysis.roles().len(),
                sentence.len(),
                "{tag} at {position}"
            );
            let counted = Role::ALL
                .iter()
                .map(|role| analysis.words_with_role(*role).count())
                .sum::<usize>();
            assert_eq!(counted, sentence.len(), "{tag} at {position}");

            // At most one terminator, and it is the last word when present.
            let terminators: Vec<usize> = analysis
                .roles()
                .iter()
                .enumerate()
                .filter(|(_, r)| **r == Role::Terminator)
                .map(|(i, _)| i)
                .collect();
            assert!(terminators.len() <= 1, "{tag} at {position}");
            assert_eq!(
                terminators.first().copied(),
                analysis.terminator_index(),
                "{tag} at {position}"
            );
        }
    }
}

/// Analysis is pure: the same slice analysed twice gives equal results, and the
/// slice is unchanged.
#[test]
fn analysis_is_pure_and_repeatable() {
    let sentence = [
        W::new("Vote", "VB"),
        W::new("for", "IN"),
        W::new("me", "PRP"),
        W::new("!", "."),
    ];
    let before: Vec<String> = sentence.iter().map(ToString::to_string).collect();

    let first = analyze(&sentence);
    let second = analyze(&sentence);
    assert_eq!(first, second);

    let after: Vec<String> = sentence.iter().map(ToString::to_string).collect();
    assert_eq!(before, after, "analysis modified the caller's sentence");
}

/// Tokens are opaque: exotic text travels through byte for byte and changes no
/// structural decision.
#[test]
fn exotic_tokens_are_carried_through_untouched() {
    let exotic = [
        "",
        " ",
        "café",
        "cafe\u{0301}",
        "Москва",
        "日本語",
        "😀",
        "a😀b",
        "\u{feff}",
        "\u{202e}",
    ];
    let sentence: Vec<W<'_>> = exotic.iter().map(|t| W::new(*t, "NN")).collect();
    let analysis = analyze(&sentence);

    assert_eq!(tokens(analysis.subject_tokens()), exotic);
    assert_eq!(analysis.subject_to_string(), exotic.join(" "));
    assert_eq!(analysis.predicate_to_string(), "");
    assert_eq!(analysis.terminator(), None);

    // The same tokens with a verb in the middle split on the verb, not on any
    // property of the text.
    let mut with_verb = sentence.clone();
    with_verb[5] = W::new(exotic[5], "VBD");
    let analysis = analyze(&with_verb);
    assert_eq!(tokens(analysis.subject_tokens()), &exotic[..5]);
    assert_eq!(tokens(analysis.predicate_tokens()), &exotic[5..]);
}

/// Joining never emits an empty slot for a word of another role.
#[test]
fn rendering_emits_no_empty_slots() {
    let sentence = [
        W::new("The", "DT"),
        W::new("bear", "NN"),
        W::new("in", "IN"),
        W::new("the", "DT"),
        W::new("valley", "NN"),
        W::new("ran", "VBD"),
        W::new(".", "."),
    ];
    let analysis = analyze(&sentence);
    assert_eq!(analysis.subject_to_string(), "The bear");
    assert_eq!(analysis.predicate_to_string(), "ran");
    for rendered in [analysis.subject_to_string(), analysis.predicate_to_string()] {
        assert!(!rendered.starts_with(' '), "{rendered:?}");
        assert!(!rendered.ends_with(' '), "{rendered:?}");
        assert!(!rendered.contains("  "), "{rendered:?}");
    }

    // An empty token still occupies a slot, so a single space is possible —
    // but only because a token is genuinely empty, never as filler.
    let empty_token = [W::new("", "NN"), W::new("", "NN")];
    assert_eq!(analyze(&empty_token).subject_to_string(), " ");
}

/// Rendering agrees with the iterators, word for word, for every role.
#[test]
fn rendering_agrees_with_the_iterators() {
    let sentence = [
        W::new("The", "DT"),
        W::new("bear", "NN"),
        W::new("chased", "VBD"),
        W::new("it", "PRP"),
        W::new(".", "."),
    ];
    let analysis: SentenceAnalysis<'_> = analyze(&sentence);
    assert_eq!(
        analysis.subject_to_string(),
        tokens(analysis.subject_tokens()).join(" ")
    );
    assert_eq!(
        analysis.predicate_to_string(),
        tokens(analysis.predicate_tokens()).join(" ")
    );
    assert_eq!(
        analysis
            .subject_words()
            .map(|w| w.tag())
            .collect::<Vec<_>>(),
        ["DT", "NN"]
    );
}

/// A long sentence stays linear and consistent; no index is ever out of range.
#[test]
fn a_long_sentence_stays_consistent() {
    let n = 20_000;
    let sentence: Vec<W<'static>> = (0..n)
        .map(|i| W::new("w", if i == 1 { "VBD" } else { "DT" }))
        .collect();
    let analysis = analyze(&sentence);

    assert_eq!(analysis.roles().len(), n);
    assert_eq!(analysis.role(0), Some(Role::Subject));
    assert_eq!(analysis.role(n - 1), Some(Role::Predicate));
    assert_eq!(analysis.subject_tokens().count(), 1);
    assert_eq!(analysis.predicate_tokens().count(), n - 1);
    // n - 1 one-byte tokens plus n - 2 separators.
    assert_eq!(analysis.predicate_to_string().len(), (n - 1) + (n - 2));
}
