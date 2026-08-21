//! The public contract of `verbora-inflectors`, asserted from outside the crate.
//!
//! Two kinds of test live here.
//!
//! **Fixtures** pin one specified behaviour each. Their expected values come
//! from the grammars cited in the crate documentation, or from arithmetic shown
//! in the test itself — never from running the code and recording what came out.
//!
//! **Regressions** pin behaviour that was wrong before the crate was specified.
//! Each carries the value the previous implementation produced, so the test is
//! demonstrably a test: it fails against that implementation and passes against
//! this one.

use verbora_inflectors::{
    CaseMode, Gender, NounInflector, NounInflectorFr, NounInflectorJa, OrdinalInflector,
    OrdinalInflectorFr, PresentVerbInflector, Rule, RuleError, SingularPluralInflector,
};

// ---------------------------------------------------------------------------
// Regressions: value before → value now
// ---------------------------------------------------------------------------

/// A rule list ordered so that a later rule can never fire is a dead rule, and a
/// dead rule means the rule that shadows it is doing work it was never meant to.
///
/// The previous English singular table opened with `(.*)ves$ → $1f`, which
/// claims **every** word ending in `-ves`, so the `ives$ → ife` rule three
/// places below it was unreachable. The visible consequence was that any noun
/// whose plural ends in `-ves` for ordinary reasons lost its `-e`.
#[test]
fn words_ending_in_ves_are_no_longer_truncated() {
    let nouns = NounInflector::new();
    // was "olif", "archif", "natif", "detectif", "jackknif", "housewif"
    assert_eq!(nouns.singularize("olives"), "olive");
    assert_eq!(nouns.singularize("archives"), "archive");
    assert_eq!(nouns.singularize("natives"), "native");
    assert_eq!(nouns.singularize("detectives"), "detective");
    assert_eq!(nouns.singularize("jackknives"), "jackknife");
    assert_eq!(nouns.singularize("housewives"), "housewife");
    assert_eq!(nouns.singularize("midwives"), "midwife");
    // The voicing plurals themselves still invert.
    assert_eq!(nouns.singularize("knives"), "knife");
    assert_eq!(nouns.singularize("wolves"), "wolf");
    assert_eq!(nouns.singularize("lives"), "life");
    assert_eq!(nouns.singularize("shelves"), "shelf");
    assert_eq!(nouns.singularize("yourselves"), "yourself");
}

/// `-man` is the noun *man* only where the word is a compound of it. The
/// previous table approximated the exception with a negative lookahead on the
/// letters `hu` occurring **anywhere** in the token, which both let through
/// every non-compound that does not contain `hu` and blocked real compounds
/// that do.
#[test]
fn the_man_exception_is_lexical_rather_than_a_substring_test() {
    let nouns = NounInflector::new();
    // Real compounds of *man*, blocked before because they contain "hu".
    assert_eq!(nouns.pluralize("huntsman"), "huntsmen"); // was "huntsmans"
    assert_eq!(nouns.pluralize("churchman"), "churchmen"); // was "churchmans"
    // Not compounds of *man*, and previously mutated anyway.
    assert_eq!(nouns.pluralize("german"), "germans"); // was "germen"
    assert_eq!(nouns.pluralize("shaman"), "shamans"); // was "shamen"
    assert_eq!(nouns.pluralize("ottoman"), "ottomans"); // was "ottomen"
    assert_eq!(nouns.pluralize("pullman"), "pullmans"); // was "pullmen"
    assert_eq!(nouns.pluralize("caiman"), "caimans"); // was "caimen"
    assert_eq!(nouns.pluralize("norman"), "normans"); // was "normen"
    assert_eq!(nouns.pluralize("roman"), "romans"); // was "romen"
    // Unchanged, and the reason the rule exists.
    assert_eq!(nouns.pluralize("man"), "men");
    assert_eq!(nouns.pluralize("woman"), "women");
    assert_eq!(nouns.pluralize("workman"), "workmen");
    assert_eq!(nouns.pluralize("human"), "humans");
    assert_eq!(nouns.pluralize("superhuman"), "superhumans");
    assert_eq!(nouns.pluralize("talisman"), "talismans");
}

/// A noun ending in `-men` that is not a plural must not be "singularised" into
/// a word that does not exist.
#[test]
fn nouns_that_merely_end_in_men_are_left_alone() {
    let nouns = NounInflector::new();
    assert_eq!(nouns.singularize("specimen"), "specimen"); // was "speciman"
    assert_eq!(nouns.singularize("acumen"), "acumen"); // was "acuman"
    assert_eq!(nouns.singularize("omen"), "omen"); // was "oman"
    assert_eq!(nouns.singularize("regimen"), "regimen");
    // Their plurals still work, and real -men plurals are untouched.
    assert_eq!(nouns.singularize("specimens"), "specimen");
    assert_eq!(nouns.singularize("workmen"), "workman");
    assert_eq!(nouns.singularize("women"), "woman");
}

/// A singular already ending in `-s` must survive `singularize`.
#[test]
fn singulars_ending_in_s_are_not_stripped() {
    let nouns = NounInflector::new();
    // was "bu", "ga", "len", "viru", "analysi", "iri", "chao"
    for word in [
        "bus", "gas", "lens", "virus", "analysis", "iris", "chaos", "atlas", "canvas", "campus",
        "status", "crisis", "dress",
    ] {
        assert_eq!(nouns.singularize(word), word, "{word}");
    }
    // And their plurals come back to them.
    assert_eq!(nouns.singularize("buses"), "bus");
    assert_eq!(nouns.singularize("gases"), "gas");
    assert_eq!(nouns.singularize("lenses"), "lens"); // was "lense"
    assert_eq!(nouns.singularize("viruses"), "virus");
}

/// `-ses` is far more often the plural of a noun in `-se`. Treating a bare `s`
/// as a sibilant took the `-e` off every one of them.
#[test]
fn a_plural_in_ses_usually_belongs_to_a_noun_in_se() {
    let nouns = NounInflector::new();
    assert_eq!(nouns.singularize("databases"), "database"); // was "databas"
    assert_eq!(nouns.singularize("houses"), "house");
    assert_eq!(nouns.singularize("cases"), "case");
    assert_eq!(nouns.singularize("noses"), "nose");
    assert_eq!(nouns.singularize("purposes"), "purpose");
}

/// Pluralising something already plural must not pluralise it twice.
#[test]
fn an_already_plural_sibilant_form_is_left_alone() {
    let nouns = NounInflector::new();
    assert_eq!(nouns.pluralize("churches"), "churches"); // was "churcheses"
    assert_eq!(nouns.pluralize("boxes"), "boxes"); // was "boxeses"
    assert_eq!(nouns.pluralize("dresses"), "dresses");
    assert_eq!(nouns.pluralize("ashes"), "ashes");
    // But a singular in the same letters still pluralises.
    assert_eq!(nouns.pluralize("church"), "churches");
    assert_eq!(nouns.pluralize("box"), "boxes");
    assert_eq!(nouns.pluralize("dress"), "dresses");
}

/// Case classification used to count UTF-16 code units and to test case with a
/// string round trip, which made a one-letter token a shout and a digit a
/// letter.
#[test]
fn case_restoration_no_longer_depends_on_utf16_or_on_uncased_characters() {
    let nouns = NounInflector::new();
    assert_eq!(nouns.pluralize("A"), "As"); // was "AS"
    assert_eq!(nouns.pluralize("I"), "Is"); // was "IS"
    assert_eq!(nouns.pluralize("1"), "1s"); // was "1S"
    assert_eq!(nouns.pluralize("👍"), "👍s");
    assert_eq!(nouns.pluralize("私"), "私s");
    // Two or more cased characters, all uppercase, is still a shout.
    assert_eq!(nouns.pluralize("URL"), "URLS");
    assert_eq!(nouns.pluralize("CHURCH"), "CHURCHES");
    assert_eq!(nouns.pluralize("Church"), "Churches");
}

/// Nothing may rewrite the case a caller supplied.
#[test]
fn interior_capitals_survive_inflection() {
    let nouns = NounInflector::new();
    assert_eq!(nouns.pluralize("iPhone"), "iPhones"); // was "iphones"
    assert_eq!(nouns.pluralize("aBC"), "aBCs"); // was "abcs"
    assert_eq!(nouns.pluralize("McDonald"), "McDonalds");
    assert_eq!(nouns.pluralize("eBay"), "eBays");
    // An invariant word comes back byte-identical whatever its case.
    assert_eq!(nouns.pluralize("dEer"), "dEer");
    assert_eq!(nouns.pluralize("DEER"), "DEER");
}

/// Lexical entries that were not what they claimed to be.
#[test]
fn misfiled_lexical_entries_are_gone() {
    let nouns = NounInflector::new();
    assert_eq!(nouns.pluralize("christmas"), "christmases"); // was "christmas"
    assert_eq!(nouns.pluralize("cloth"), "cloths"); // was "clothes"
    assert_eq!(nouns.pluralize("torso"), "torsos"); // was "torsi"
    assert_eq!(nouns.pluralize("virus"), "viruses"); // was "viri"
    assert_eq!(nouns.pluralize("fife"), "fifes"); // was "fives"
    assert_eq!(nouns.pluralize("strife"), "strifes"); // was "strives"
    assert_eq!(nouns.pluralize("sex"), "sexes");
}

/// The modal auxiliaries have no third-person singular form, and a plain form
/// that happens to end in `-ed` still needs one.
#[test]
fn verb_agreement_covers_modals_and_bare_ed_stems() {
    let verbs = PresentVerbInflector::new();
    for modal in ["can", "may", "must", "shall", "will", "ought"] {
        assert_eq!(verbs.singularize(modal), modal, "{modal}"); // was "cans", "mays", …
        assert_eq!(verbs.pluralize(modal), modal, "{modal}");
    }
    assert_eq!(verbs.singularize("need"), "needs"); // was "need"
    assert_eq!(verbs.singularize("feed"), "feeds"); // was "feed"
    assert_eq!(verbs.singularize("proceed"), "proceeds");
    // A genuine past form still has no present third-person singular.
    assert_eq!(verbs.singularize("watched"), "watched");
    // The four verbs whose plain form ends in -ie.
    assert_eq!(verbs.pluralize("dies"), "die"); // was "dy"
    assert_eq!(verbs.pluralize("lies"), "lie"); // was "ly"
    assert_eq!(verbs.pluralize("ties"), "tie"); // was "ty"
    assert_eq!(verbs.pluralize("flies"), "fly");
}

/// A French singular ending in `-z` is invariant by the same rule as `-s` and
/// `-x`; the previous plural rule listed only the first two, so a `-z` noun
/// outside the lexical list gained an `s`.
#[test]
fn french_invariance_covers_z() {
    let fr = NounInflectorFr::new();
    assert_eq!(fr.pluralize("blitz"), "blitz"); // was "blitzs"
    assert_eq!(fr.pluralize("spitz"), "spitz"); // was "spitzs"
    assert_eq!(fr.pluralize("gaz"), "gaz");
    assert_eq!(fr.pluralize("quartz"), "quartz");
    assert_eq!(fr.pluralize("prix"), "prix");
    assert_eq!(fr.pluralize("abus"), "abus");
}

/// Ordinals of negative integers follow the numeral, not the sign of a
/// remainder.
#[test]
fn negative_ordinals_take_the_suffix_of_their_magnitude() {
    assert_eq!(OrdinalInflector::nth(-1), "-1st"); // was "-1th"
    assert_eq!(OrdinalInflector::nth(-2), "-2nd"); // was "-2th"
    assert_eq!(OrdinalInflector::nth(-3), "-3rd"); // was "-3th"
    assert_eq!(OrdinalInflector::nth(-21), "-21st"); // was "-21th"
    assert_eq!(OrdinalInflector::nth(-11), "-11th");
}

// ---------------------------------------------------------------------------
// Fixtures from the cited grammars
// ---------------------------------------------------------------------------

/// English plural classes, one fixture per rule class in the table.
#[test]
fn english_plural_classes() {
    let n = NounInflector::new();
    // Regular -s, and the -y spelling rule.
    assert_eq!(n.pluralize("hacker"), "hackers");
    assert_eq!(n.pluralize("day"), "days");
    assert_eq!(n.pluralize("journey"), "journeys");
    assert_eq!(n.pluralize("party"), "parties");
    // Sibilant stems take -es.
    assert_eq!(n.pluralize("church"), "churches");
    assert_eq!(n.pluralize("bush"), "bushes");
    assert_eq!(n.pluralize("quiz"), "quizzes");
    // Fricative voicing.
    assert_eq!(n.pluralize("wolf"), "wolves");
    assert_eq!(n.pluralize("shelf"), "shelves");
    assert_eq!(n.pluralize("knife"), "knives");
    assert_eq!(n.pluralize("wife"), "wives");
    assert_eq!(n.pluralize("gulf"), "gulfs");
    // Nouns in -o that take -oes.
    assert_eq!(n.pluralize("tomato"), "tomatoes");
    assert_eq!(n.pluralize("hero"), "heroes");
    assert_eq!(n.pluralize("photo"), "photos");
    // Mutation and -en plurals.
    assert_eq!(n.pluralize("foot"), "feet");
    assert_eq!(n.pluralize("mouse"), "mice");
    assert_eq!(n.pluralize("ox"), "oxen");
    assert_eq!(n.pluralize("child"), "children");
    assert_eq!(n.pluralize("person"), "people");
    // Latin and Greek plurals.
    assert_eq!(n.pluralize("cactus"), "cacti");
    assert_eq!(n.pluralize("radius"), "radii");
    assert_eq!(n.pluralize("formula"), "formulae");
    assert_eq!(n.pluralize("matrix"), "matrices");
    assert_eq!(n.pluralize("index"), "indices");
    assert_eq!(n.pluralize("basis"), "bases");
    assert_eq!(n.pluralize("crisis"), "crises");
    assert_eq!(n.pluralize("axis"), "axes");
    assert_eq!(n.pluralize("curriculum"), "curricula");
    assert_eq!(n.pluralize("criterion"), "criteria");
    // Zero plurals.
    assert_eq!(n.pluralize("sheep"), "sheep");
    assert_eq!(n.pluralize("aircraft"), "aircraft");
    assert_eq!(n.pluralize("offspring"), "offspring");
}

#[test]
fn english_singular_classes() {
    let n = NounInflector::new();
    assert_eq!(n.singularize("hackers"), "hacker");
    assert_eq!(n.singularize("parties"), "party");
    assert_eq!(n.singularize("movies"), "movie");
    assert_eq!(n.singularize("churches"), "church");
    assert_eq!(n.singularize("boxes"), "box");
    assert_eq!(n.singularize("quizzes"), "quiz");
    assert_eq!(n.singularize("wolves"), "wolf");
    assert_eq!(n.singularize("tomatoes"), "tomato");
    assert_eq!(n.singularize("feet"), "foot");
    assert_eq!(n.singularize("mice"), "mouse");
    assert_eq!(n.singularize("oxen"), "ox");
    assert_eq!(n.singularize("children"), "child");
    assert_eq!(n.singularize("people"), "person");
    assert_eq!(n.singularize("cacti"), "cactus");
    assert_eq!(n.singularize("formulae"), "formula");
    assert_eq!(n.singularize("matrices"), "matrix");
    assert_eq!(n.singularize("vertices"), "vertex");
    assert_eq!(n.singularize("appendices"), "appendix");
    assert_eq!(n.singularize("parentheses"), "parenthesis");
    assert_eq!(n.singularize("analyses"), "analysis");
    assert_eq!(n.singularize("criteria"), "criterion");
    assert_eq!(n.singularize("curricula"), "curriculum");
    assert_eq!(n.singularize("sheep"), "sheep");
    // Non-plurals come back untouched.
    assert_eq!(n.singularize("hacker"), "hacker");
    assert_eq!(n.singularize("table"), "table");
}

#[test]
fn french_plural_classes() {
    let f = NounInflectorFr::new();
    assert_eq!(f.pluralize("orange"), "oranges");
    assert_eq!(f.pluralize("cheval"), "chevaux");
    assert_eq!(f.pluralize("carnaval"), "carnavals");
    assert_eq!(f.pluralize("travail"), "travaux");
    assert_eq!(f.pluralize("bijou"), "bijoux");
    assert_eq!(f.pluralize("trou"), "trous");
    assert_eq!(f.pluralize("cadeau"), "cadeaux");
    assert_eq!(f.pluralize("landau"), "landaus");
    assert_eq!(f.pluralize("pneu"), "pneus");
    assert_eq!(f.pluralize("cheveu"), "cheveux");
    assert_eq!(f.pluralize("œil"), "yeux");
    assert_eq!(f.pluralize("madame"), "mesdames");
    assert_eq!(f.pluralize("monsieur"), "messieurs");
    assert_eq!(f.pluralize("rhinocéros"), "rhinocéros");
}

#[test]
fn french_singular_classes() {
    let f = NounInflectorFr::new();
    assert_eq!(f.singularize("chats"), "chat");
    assert_eq!(f.singularize("chevaux"), "cheval");
    assert_eq!(f.singularize("travaux"), "travail");
    assert_eq!(f.singularize("tuyaux"), "tuyau");
    assert_eq!(f.singularize("bijoux"), "bijou");
    assert_eq!(f.singularize("cadeaux"), "cadeau");
    assert_eq!(f.singularize("cheveux"), "cheveu");
    assert_eq!(f.singularize("aïeux"), "aïeul");
    assert_eq!(f.singularize("bisaïeux"), "bisaïeul");
    assert_eq!(f.singularize("apparaux"), "appareil");
    assert_eq!(f.singularize("yeux"), "œil");
    assert_eq!(f.singularize("mesdames"), "madame");
    // Invariant singulars stay put.
    assert_eq!(f.singularize("abus"), "abus");
    assert_eq!(f.singularize("rhinocéros"), "rhinocéros");
    // `baux` is the plural of `bail`, not of a stem `b` plus `-aux`.
    assert_eq!(f.singularize("baux"), "bail");
}

#[test]
fn japanese_suffixation_and_its_exceptions() {
    let j = NounInflectorJa::new();
    assert_eq!(j.pluralize("私"), "私たち");
    assert_eq!(j.pluralize("人"), "人々");
    assert_eq!(j.pluralize("友達"), "友達");
    assert_eq!(j.singularize("人たち"), "人");
    assert_eq!(j.singularize("私達"), "私");
    assert_eq!(j.singularize("野郎共"), "野郎");
    assert_eq!(j.singularize("先生方"), "先生");
    assert_eq!(j.singularize("人々"), "人");
    assert_eq!(j.singularize("人人"), "人");
    // Words that merely end in a plural suffix.
    assert_eq!(j.singularize("かたち"), "かたち");
    assert_eq!(j.singularize("配達"), "配達");
    assert_eq!(j.singularize("平等"), "平等");
    assert_eq!(j.singularize("友達"), "友達");
}

#[test]
fn english_verb_agreement() {
    let v = PresentVerbInflector::new();
    assert_eq!(v.singularize("run"), "runs");
    assert_eq!(v.singularize("catch"), "catches");
    assert_eq!(v.singularize("pass"), "passes");
    assert_eq!(v.singularize("annex"), "annexes");
    assert_eq!(v.singularize("go"), "goes");
    assert_eq!(v.singularize("buzz"), "buzzes");
    assert_eq!(v.singularize("fly"), "flies");
    assert_eq!(v.singularize("play"), "plays");
    assert_eq!(v.singularize("be"), "is");
    assert_eq!(v.singularize("am"), "am");
    assert_eq!(v.singularize("is"), "is");
    assert_eq!(v.singularize("has"), "has");
    assert_eq!(v.singularize("was"), "was");
    assert_eq!(v.singularize("are"), "is");
    assert_eq!(v.singularize("have"), "has");
    assert_eq!(v.singularize("were"), "was");

    assert_eq!(v.pluralize("runs"), "run");
    assert_eq!(v.pluralize("catches"), "catch");
    assert_eq!(v.pluralize("passes"), "pass");
    assert_eq!(v.pluralize("annexes"), "annex");
    assert_eq!(v.pluralize("goes"), "go");
    assert_eq!(v.pluralize("buzzes"), "buzz");
    assert_eq!(v.pluralize("flies"), "fly");
    assert_eq!(v.pluralize("makes"), "make");
    assert_eq!(v.pluralize("is"), "are");
    assert_eq!(v.pluralize("am"), "are");
    assert_eq!(v.pluralize("has"), "have");
    assert_eq!(v.pluralize("was"), "were");
}

// ---------------------------------------------------------------------------
// Totality, determinism and the buffer API
// ---------------------------------------------------------------------------

/// Awkward input across every inflector and both directions. Nothing here
/// asserts a linguistic claim; it asserts that the crate is total.
#[test]
fn every_inflector_is_total_on_awkward_input() {
    let inputs = [
        "",
        " ",
        "\t\n",
        "\u{2028}",
        "a",
        "A",
        "1",
        "-",
        "---",
        "\u{0}",
        "ß",
        "İ",
        "ﬁ",
        "👍",
        "👍👍👍",
        "a\u{300}",
        "\u{300}",
        "私",
        "ＡＢＣ",
        "e\u{301}s",
        "s",
        "es",
        "S",
        "'",
        "don't",
        "ab\rcd",
        "ab\ncd",
        "very-long-hyphenated-token-that-no-rule-claims",
        "ΑΣ",
        "ας",
    ];
    let mut buffer = String::new();
    for input in inputs {
        for (label, plural, singular) in [
            (
                "en",
                NounInflector::new().pluralize(input),
                NounInflector::new().singularize(input),
            ),
            (
                "fr",
                NounInflectorFr::new().pluralize(input),
                NounInflectorFr::new().singularize(input),
            ),
            (
                "ja",
                NounInflectorJa::new().pluralize(input),
                NounInflectorJa::new().singularize(input),
            ),
            (
                "verb",
                PresentVerbInflector::new().pluralize(input),
                PresentVerbInflector::new().singularize(input),
            ),
        ] {
            // The empty token is the only input that may produce nothing.
            if input.is_empty() {
                assert!(plural.is_empty() && singular.is_empty(), "{label}");
            } else {
                assert!(!plural.is_empty(), "{label} pluralize({input:?}) is empty");
                assert!(
                    !singular.is_empty(),
                    "{label} singularize({input:?}) is empty"
                );
            }
            // The buffer form agrees with the allocating form.
            if label == "en" {
                buffer.clear();
                NounInflector::new().pluralize_into(input, &mut buffer);
                assert_eq!(buffer, plural, "buffer form diverged for {input:?}");
            }
        }
    }
}

/// Determinism, stated as an assertion rather than assumed: two independent
/// instances, called in different orders, agree on every input.
#[test]
fn repeated_calls_and_fresh_instances_agree() {
    let inputs = [
        "party",
        "wolf",
        "criterion",
        "deer",
        "children",
        "IBM",
        "iPhone",
        "私",
        "cheval",
        "",
    ];
    let first = NounInflector::new();
    let second = NounInflector::new();
    for input in inputs {
        let a = first.pluralize(input);
        let b = second.pluralize(input);
        let c = first.pluralize(input);
        assert_eq!(a, b, "{input:?}");
        assert_eq!(a, c, "{input:?}");
    }
}

#[test]
fn the_buffer_form_and_the_allocating_form_always_agree() {
    let corpus = [
        "party",
        "wolf",
        "criterion",
        "deer",
        "children",
        "",
        "IBM",
        "iPhone",
        "私",
    ];
    for inflector_output in [true, false] {
        let mut buffer = String::new();
        for word in corpus {
            buffer.clear();
            let owned = if inflector_output {
                NounInflector::new().pluralize(word)
            } else {
                NounInflector::new().singularize(word)
            };
            if inflector_output {
                NounInflector::new().pluralize_into(word, &mut buffer);
            } else {
                NounInflector::new().singularize_into(word, &mut buffer);
            }
            assert_eq!(buffer, owned, "{word:?}");
        }
    }
}

#[test]
fn instances_do_not_share_caller_additions() {
    let mut a = NounInflector::new();
    a.add_plural(Rule::new("(?i)^gizmo$", "gizmoz").unwrap());
    a.add_irregular("thingy", "thingies");
    let b = NounInflector::new();

    assert_eq!(a.pluralize("gizmo"), "gizmoz");
    assert_eq!(a.pluralize("thingy"), "thingies");
    assert_eq!(b.pluralize("gizmo"), "gizmos");
    assert_eq!(b.pluralize("thingy"), "thingies"); // the regular -y rule agrees here
    assert_eq!(b.singularize("gizmoz"), "gizmoz");
}

#[test]
fn rule_construction_refuses_what_it_cannot_expand() {
    // The whole reason `Rule::new` returns a Result.
    assert!(matches!(
        Rule::new("(a)", "$1s"),
        Err(RuleError::BareGroupReference { .. })
    ));
    assert!(matches!(
        Rule::new("(a)", "${9}"),
        Err(RuleError::UnknownGroup { .. })
    ));
    assert!(matches!(
        Rule::new("(?P<x>a", "y"),
        Err(RuleError::Pattern { .. })
    ));
    // And the shape that works.
    let rule = Rule::new("(?i)(ware)$", "${1}z").unwrap();
    assert_eq!(rule.apply("software").as_deref(), Some("softwarez"));
    assert_eq!(rule.pattern(), "(?i)(ware)$");
}

#[test]
fn case_mode_is_the_documented_classification() {
    assert_eq!(CaseMode::of("word"), CaseMode::Preserve);
    assert_eq!(CaseMode::of("Word"), CaseMode::Title);
    assert_eq!(CaseMode::of("WORD"), CaseMode::Upper);
    assert_eq!(CaseMode::of("W"), CaseMode::Title);
    assert_eq!(CaseMode::of(""), CaseMode::Preserve);
    assert_eq!(CaseMode::Title.apply("child"), "Child");
    assert_eq!(CaseMode::Upper.apply("child"), "CHILD");
    assert_eq!(CaseMode::Preserve.apply("cHild"), "cHild");

    let mut buffer = String::from("a ");
    CaseMode::Title.apply_into("child", &mut buffer);
    assert_eq!(buffer, "a Child");
}

#[test]
fn ordinals_agree_with_arithmetic_shown_here() {
    // suffix(n) is decided by n.abs() % 100; spell that out rather than
    // restating the table.
    for n in [
        0i64, 1, 2, 3, 4, 10, 11, 12, 13, 14, 20, 21, 100, 111, 1_000_000,
    ] {
        let last_two = n.unsigned_abs() % 100;
        let expected = if (11..=13).contains(&last_two) {
            "th"
        } else {
            match last_two % 10 {
                1 => "st",
                2 => "nd",
                3 => "rd",
                _ => "th",
            }
        };
        assert_eq!(OrdinalInflector::suffix(n), expected, "{n}");
        assert_eq!(OrdinalInflector::nth(n), format!("{n}{expected}"));
    }
    assert_eq!(OrdinalInflectorFr::nth(1, Gender::Masculine), "1er");
    assert_eq!(OrdinalInflectorFr::nth(1, Gender::Feminine), "1re");
    assert_eq!(OrdinalInflectorFr::nth(21, Gender::Feminine), "21e");
    assert_eq!(OrdinalInflectorFr::suffix(0, Gender::Masculine), "e");
}

#[test]
fn generic_code_can_drive_every_inflector() {
    fn round_trip<I: SingularPluralInflector>(inflector: &I, singular: &str) -> String {
        inflector.singularize(&inflector.pluralize(singular))
    }
    assert_eq!(round_trip(&NounInflector::new(), "party"), "party");
    assert_eq!(round_trip(&NounInflector::new(), "wolf"), "wolf");
    assert_eq!(round_trip(&NounInflector::new(), "child"), "child");
    assert_eq!(round_trip(&NounInflectorFr::new(), "cheval"), "cheval");
    assert_eq!(round_trip(&NounInflectorJa::new(), "私"), "私");
}
