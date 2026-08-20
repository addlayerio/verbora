//! The built-in rule tables, and the sources that define them.
//!
//! Every table here is one of three kinds, and each kind has a different burden
//! of proof:
//!
//! * **A rule** states a productive pattern of the language. It cites the
//!   grammar that describes the pattern, and it carries a *witness*: a token
//!   that must reach this rule and no earlier one. `tests` in
//!   [`crate::engine`] walks every rule of every table, checks that its witness
//!   is not claimed by an earlier rule, and checks that the rule produces the
//!   recorded form. A rule with no reachable witness is dead and is deleted, not
//!   documented.
//! * **A lexical list** enumerates words a rule cannot derive — English zero
//!   plurals, French singulars that already end in `-s`, Japanese words that
//!   merely look like `-たち` plurals. A list is never exhaustive and never
//!   claims to be; what it must be is *checkable*, so each list has a stated
//!   membership criterion and a test that enforces it.
//! * **An irregular pair** is a single suppletive or mutated form, listed
//!   because no rule generates it.
//!
//! Sources, once, for the whole module:
//!
//! * **English** — Quirk, Greenbaum, Leech & Svartvik, *A Comprehensive Grammar
//!   of the English Language* (Longman, 1985), ch. 5, on the number of nouns:
//!   regular `-s`/`-es`, the `-y → -ies` spelling rule, fricative voicing
//!   (`-f`/`-fe → -ves`), nouns in `-o`, mutation plurals (`foot`/`feet`),
//!   `-en` plurals (`ox`/`oxen`, `child`/`children`), zero plurals, and the
//!   Latin and Greek plurals of loan words. Huddleston & Pullum, *The Cambridge
//!   Grammar of the English Language* (CUP, 2002), ch. 18, covers the same
//!   inflection from a different angle and is the cross-check used where the two
//!   descriptions could differ.
//! * **French** — Grevisse & Goosse, *Le Bon Usage*, on *le pluriel des noms*:
//!   the general `-s`, the invariance of singulars already ending in `-s`, `-x`
//!   or `-z`, `-al → -aux`, `-ail → -aux` for a closed list, `-au`/`-eau`/`-eu`
//!   → `-x`, and the `-ou → -oux` seven. Individual lexical entries follow the
//!   *Dictionnaire de l'Académie française*, 9th edition.
//! * **Japanese** — Martin, *A Reference Grammar of Japanese* (Yale UP, 1975)
//!   and Shibatani, *The Languages of Japan* (CUP, 1990), on the plural and
//!   associative suffixes 〜たち, 〜達, 〜等, 〜共/〜ども and 〜方/〜がた, and on
//!   the fact that Japanese does not mark number obligatorily. The iteration
//!   mark 々 (U+3005) is the conventional spelling of a reduplicated noun.

/// One entry of an ordered rule table.
pub(crate) struct RawRule {
    /// A `regex`-crate pattern. Case-insensitive rules spell `(?i)` themselves.
    pub(crate) pattern: &'static str,
    /// The replacement template, or `None` for a guard that matches and stops.
    pub(crate) replacement: Option<&'static str>,
    /// A lowercase token that must reach this rule and no earlier one.
    ///
    /// Read only by the reachability enumeration in [`crate::engine`]'s tests;
    /// it is the proof obligation attached to the rule, not runtime data.
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) witness: &'static str,
    /// What the whole table must turn [`Self::witness`] into.
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) expected: &'static str,
}

/// Shorthand for a rewriting rule.
const fn rule(
    pattern: &'static str,
    replacement: &'static str,
    witness: &'static str,
    expected: &'static str,
) -> RawRule {
    RawRule {
        pattern,
        replacement: Some(replacement),
        witness,
        expected,
    }
}

/// Shorthand for a guard: matches, and the token is its own answer.
const fn guard(pattern: &'static str, witness: &'static str) -> RawRule {
    RawRule {
        pattern,
        replacement: None,
        witness,
        expected: witness,
    }
}

/// English nouns.
pub(crate) mod en {
    use super::{RawRule, guard, rule};

    /// Nouns with no distinct plural: zero plurals (chiefly animals taken
    /// collectively), non-count nouns, and singulars that already end in `-s`.
    ///
    /// Membership criterion: the word has one form for both numbers in standard
    /// English. The list is lexical and deliberately not exhaustive — a noun
    /// absent from it takes the regular plural.
    pub(crate) static INVARIANT: &[&str] = &[
        "aircraft",
        "barracks",
        "billiards",
        "bison",
        "bream",
        "carp",
        "chassis",
        "cod",
        "corps",
        "crossroads",
        "darts",
        "debris",
        "deer",
        "diabetes",
        "elk",
        "equipment",
        "fish",
        "flounder",
        "gallows",
        "graffiti",
        "grouse",
        "headquarters",
        "herpes",
        "homework",
        "information",
        "innings",
        "mackerel",
        "means",
        "measles",
        "mews",
        "money",
        "moose",
        "mumps",
        "news",
        "offspring",
        "rabies",
        "reindeer",
        "rice",
        "salmon",
        "series",
        "sheep",
        "shrimp",
        "species",
        "swine",
        "tennis",
        "trout",
        "tuna",
        "whiting",
        "wildebeest",
    ];

    /// Singular → plural for forms no rule derives: `-en` plurals, mutation
    /// plurals, one suppletion (`person`/`people`) and one Greek `-is/-ides`.
    pub(crate) static PLURAL_IRREGULAR: &[(&str, &str)] = &[
        ("child", "children"),
        ("ephemeris", "ephemerides"),
        ("foot", "feet"),
        ("goose", "geese"),
        ("louse", "lice"),
        ("mouse", "mice"),
        ("ox", "oxen"),
        ("person", "people"),
        ("tooth", "teeth"),
    ];

    /// The same pairs, keyed by the plural.
    pub(crate) static SINGULAR_IRREGULAR: &[(&str, &str)] = &[
        ("children", "child"),
        ("ephemerides", "ephemeris"),
        ("feet", "foot"),
        ("geese", "goose"),
        ("lice", "louse"),
        ("mice", "mouse"),
        ("oxen", "ox"),
        ("people", "person"),
        ("teeth", "tooth"),
    ];

    /// Singular → plural. First match wins.
    pub(crate) static PLURAL_REGULAR: &[RawRule] = &[
        // A token already in the `-es` plural shape is left alone; without this
        // the sibilant rule below would make `dresses` into `dresseses`.
        guard("(?i)(ses|xes|zes|ches|shes)$", "dresses"),
        // -y after a vowel is a plain -s; after a consonant it is -ies.
        rule("(?i)([aeiou])y$", "${1}ys", "day", "days"),
        rule("(?i)y$", "ies", "party", "parties"),
        // Fricative voicing. Both stem lists are closed: a word ending in these
        // letters but built on another morpheme (`fife`, `gulf`) is regular.
        rule("(?i)(kni|li|wi)fe$", "${1}ves", "knife", "knives"),
        // `dwar` is absent here and present in the singular table on purpose:
        // both `dwarfs` and `dwarves` are attested, so Verbora emits the one
        // dictionaries lead with and accepts the other on the way back.
        rule(
            "(?i)(cal|el|hal|hoo|lea|loa|scar|sel|shea|shel|thie|wol)f$",
            "${1}ves",
            "wolf",
            "wolves",
        ),
        // Latin and Greek plurals of loan words, each on a closed stem list.
        rule(
            "(?i)(alumn|antenn|formul|larv|nebul|vertebr|vit)a$",
            "${1}ae",
            "formula",
            "formulae",
        ),
        rule(
            "(?i)(alumn|bacill|cact|calcul|foc|fung|hippopotam|nucle|octop|radi|stimul|syllab)us$",
            "${1}i",
            "cactus",
            "cacti",
        ),
        rule(
            "(?i)(buffal|domin|ech|embarg|her|mosquit|potat|tomat|tornad|torped|vet|volcan)o$",
            "${1}oes",
            "tomato",
            "tomatoes",
        ),
        // -sis is productive across the -osis/-esis/-ysis families.
        rule("(?i)sis$", "ses", "basis", "bases"),
        rule("(?i)(ax|test)is$", "${1}es", "axis", "axes"),
        rule(
            "(?i)(bacteri|curricul|dat|errat|medi|memorand|ov|strat|symposi)um$",
            "${1}a",
            "curriculum",
            "curricula",
        ),
        rule(
            "(?i)(criteri|phenomen)on$",
            "${1}a",
            "criterion",
            "criteria",
        ),
        rule(
            "(?i)(append|cort|ind|matr|vert)(?:ix|ex)$",
            "${1}ices",
            "matrix",
            "matrices",
        ),
        // `-man` is the noun *man* only in compounds of it. These nouns end in
        // the same three letters for unrelated reasons and are regular, so they
        // claim the token before the mutation rule can.
        rule(
            "(?i)(caiman|cayman|desman|dolman|dragoman|firman|german|human|leman|liman|norman|ottoman|pullman|roman|shaman|talisman)$",
            "${1}s",
            "human",
            "humans",
        ),
        rule("(?i)man$", "men", "workman", "workmen"),
        // A monosyllable in a single vowel plus `z` doubles the `z`.
        rule("(?i)^(fez|quiz|whiz)$", "${1}zes", "quiz", "quizzes"),
        // A stem ending in a sibilant takes -es.
        rule("(?i)(s|x|z|ch|sh)$", "${1}es", "church", "churches"),
        // Everything else takes -s.
        rule("$", "s", "hacker", "hackers"),
    ];

    /// Plural → singular. First match wins.
    pub(crate) static SINGULAR_REGULAR: &[RawRule] = &[
        // Nouns that end in `-men` without being plurals of anything.
        guard(
            "(?i)^(?:abdomen|acumen|albumen|bitumen|catechumen|cerumen|cyclamen|hymen|lumen|omen|regimen|rumen|semen|specimen|stamen)$",
            "specimen",
        ),
        // The voicing rules, inverted. `-lives` needs the guard against a
        // preceding vowel so that `olives` is not read as a compound of `life`.
        rule("(?i)knives$", "knife", "jackknives", "jackknife"),
        rule("(?i)wives$", "wife", "housewives", "housewife"),
        rule("(?i)(^|[^aeiou])lives$", "${1}life", "lives", "life"),
        rule(
            "(?i)(cal|dwar|el|hal|hoo|lea|loa|scar|sel|shea|shel|thie|wol)ves$",
            "${1}f",
            "wolves",
            "wolf",
        ),
        // `-ies → -y`, except after `v`, where the singular ends in `-ie`
        // (`movies`, `stevies`) and the final `-s` alone comes off.
        rule("(?i)([^v])ies$", "${1}y", "parties", "party"),
        rule(
            "(?i)(alumn|antenn|formul|larv|nebul|vertebr|vit)ae$",
            "${1}a",
            "formulae",
            "formula",
        ),
        rule(
            "(?i)(alumn|bacill|cact|calcul|foc|fung|hippopotam|nucle|octop|radi|stimul|syllab)i$",
            "${1}us",
            "cacti",
            "cactus",
        ),
        rule(
            "(?i)(buffal|domin|ech|embarg|her|mosquit|potat|tomat|tornad|torped|vet|volcan)oes$",
            "${1}o",
            "tomatoes",
            "tomato",
        ),
        // Anchored, unlike its plural counterpart: `-ses` is only a `-sis`
        // plural for these whole words, and leaving it open would turn
        // `databases` into `databasis`.
        rule(
            "(?i)^(analy|cri|diagno|ellip|empha|hypothe|metamorpho|neuro|oa|paraly|parenthe|progno|psycho|synop|the)ses$",
            "${1}sis",
            "parentheses",
            "parenthesis",
        ),
        rule(
            "(?i)(bacteri|curricul|dat|errat|medi|memorand|ov|strat|symposi)a$",
            "${1}um",
            "curricula",
            "curriculum",
        ),
        rule(
            "(?i)(criteri|phenomen)a$",
            "${1}on",
            "criteria",
            "criterion",
        ),
        rule("(?i)(append|matr)ices$", "${1}ix", "matrices", "matrix"),
        rule("(?i)(cort|ind|vert)ices$", "${1}ex", "vertices", "vertex"),
        rule("(?i)^(fez|quiz|whiz)zes$", "${1}", "quizzes", "quiz"),
        // Nouns whose singular already ends in `-s` and whose plural therefore
        // adds `-es`. Without the list, `buses` would lose only its final `s`.
        rule(
            "(?i)^(atlas|bias|bonus|bus|campus|canvas|circus|gas|iris|lens|plus|status|surplus|virus)es$",
            "${1}",
            "buses",
            "bus",
        ),
        // Bare `s` is deliberately absent from this class: `-ses` is far more
        // often the plural of a noun in `-se` (`database`, `house`, `case`) than
        // of one in `-s`, and those are handled by the final rule.
        rule("(?i)(ch|sh|ss|x|z)es$", "${1}", "churches", "church"),
        rule("(?i)men$", "man", "workmen", "workman"),
        // Singulars in `-ss`, `-us` and `-is`. No English plural ends in any of
        // the three, so these three shapes are safe to protect wholesale.
        guard("(?i)(?:ss|us|is)$", "dress"),
        // Singulars in `-as` and `-os` have to be listed: `-as` and `-os` are
        // both common plural endings (`cameras`, `photos`).
        guard(
            "(?i)^(?:alias|atlas|bias|canvas|chaos|cosmos|gas|lens|pathos|rhinoceros)$",
            "gas",
        ),
        rule("(?i)s$", "", "cats", "cat"),
    ];
}

/// French nouns.
pub(crate) mod fr {
    use super::{RawRule, guard, rule};

    include!("data_fr_invariant.rs");

    /// Singular → plural for forms no rule derives.
    pub(crate) static PLURAL_IRREGULAR: &[(&str, &str)] = &[
        ("ail", "aulx"),
        ("bonhomme", "bonshommes"),
        ("bétail", "bestiaux"),
        ("ciel", "cieux"),
        ("madame", "mesdames"),
        ("mademoiselle", "mesdemoiselles"),
        ("mafioso", "mafiosi"),
        ("monsieur", "messieurs"),
        ("putto", "putti"),
        ("targui", "touareg"),
        ("œil", "yeux"),
    ];

    /// The same pairs, keyed by the plural.
    pub(crate) static SINGULAR_IRREGULAR: &[(&str, &str)] = &[
        ("aulx", "ail"),
        ("bestiaux", "bétail"),
        ("bonshommes", "bonhomme"),
        ("cieux", "ciel"),
        ("mafiosi", "mafioso"),
        ("mesdames", "madame"),
        ("mesdemoiselles", "mademoiselle"),
        ("messieurs", "monsieur"),
        ("putti", "putto"),
        ("touareg", "targui"),
        ("yeux", "œil"),
    ];

    /// Singular → plural. First match wins.
    pub(crate) static PLURAL_REGULAR: &[RawRule] = &[
        // The nouns in `-al` that keep the regular `-s`.
        rule(
            "(?i)^(av|b|c|carnav|cérémoni|chac|corr|emment|emmenth|festiv|fut|gavi|gra|narv|p|récit|rég|rit|rorqu|st)al$",
            "${1}als",
            "carnaval",
            "carnavals",
        ),
        // The seven-plus nouns in `-ail` that take `-aux`.
        rule(
            "(?i)^(aspir|b|cor|ém|ferm|gemm|soupir|trav|vant|vent|vitr)ail$",
            "${1}aux",
            "travail",
            "travaux",
        ),
        // The nouns in `-ou` that take `-oux`.
        rule(
            "(?i)^(bij|caill|ch|gen|hib|jouj|p|rip|chouch)ou$",
            "${1}oux",
            "bijou",
            "bijoux",
        ),
        // The nouns in `-au` and `-eu` that keep the regular `-s`.
        rule(
            "(?i)^(gr|berimb|don|karb|land|pil|rest|sarr|un)au$",
            "${1}aus",
            "landau",
            "landaus",
        ),
        rule("(?i)^(bl|ém|enf|pn)eu$", "${1}eus", "pneu", "pneus"),
        // Otherwise `-au`, `-eau`, `-eu` and `-œu` take `-x`.
        rule("(?i)(au|eau|eu|œu)$", "${1}x", "cadeau", "cadeaux"),
        rule("(?i)al$", "aux", "cheval", "chevaux"),
        // A singular already ending in `-s`, `-x` or `-z` is invariant. The
        // lexical list above only matters for the singular direction; here the
        // rule is enough, and it covers `-z`, which no list needs to enumerate.
        guard("(?i)(s|x|z)$", "blitz"),
        rule("$", "s", "orange", "oranges"),
    ];

    /// Plural → singular. First match wins.
    pub(crate) static SINGULAR_REGULAR: &[RawRule] = &[
        rule(
            "(?i)^(aspir|b|cor|ém|ferm|gemm|soupir|trav|vant|vent|vitr)aux$",
            "${1}ail",
            "travaux",
            "travail",
        ),
        // `b` is absent here on purpose: `baux` is the plural of `bail`, which
        // the rule above already claims, so a `b` alternative would be dead.
        rule(
            "(?i)^(aloy|bouc|boy|burg|conoy|coy|cr|esquim|ét|fabli|flé|flûti|glu|gr|gru|hoy|joy|kérab|matéri|nobli|noy|pré|sen|sén|t|touch|tuss|tuy|v|ypré)aux$",
            "${1}au",
            "tuyaux",
            "tuyau",
        ),
        rule(
            "(?i)^(bij|caill|ch|gen|hib|jouj|p|rip|chouch)oux$",
            "${1}ou",
            "bijoux",
            "bijou",
        ),
        rule("(?i)^(bis)?aïeux$", "${1}aïeul", "aïeux", "aïeul"),
        rule("(?i)^apparaux$", "appareil", "apparaux", "appareil"),
        rule("(?i)^ciels$", "ciel", "ciels", "ciel"),
        rule("(?i)^œils$", "œil", "œils", "œil"),
        rule("(?i)(eau|eu|œu)x$", "${1}", "cadeaux", "cadeau"),
        rule("(?i)aux$", "al", "chevaux", "cheval"),
        rule("(?i)s$", "", "chats", "chat"),
    ];
}

/// Japanese nouns.
pub(crate) mod ja {
    use super::{RawRule, rule};

    /// Words that already denote a group and take no further suffix.
    pub(crate) static PLURAL_INVARIANT: &[&str] = &[
        "ともだち",
        "友だち",
        "友達",
        "女友達",
        "学校友達",
        "幼友達",
        "男友達",
        "茶飲み友達",
        "遊び友達",
        "酒飲み友達",
        "飲み友達",
    ];

    /// Words that end in 〜たち, 〜達 or 〜等 without being suffixed plurals.
    ///
    /// Membership criterion: the word is a single lexeme whose final characters
    /// coincide with a plural suffix — かたち "shape", 配達 "delivery", 平等
    /// "equality" — so stripping the suffix would produce a non-word.
    pub(crate) static SINGULAR_INVARIANT: &[&str] = &[
        "いたち",
        "いでたち",
        "おいたち",
        "かおかたち",
        "かたち",
        "からたち",
        "ついたち",
        "なりかたち",
        "なりたち",
        "はたち",
        "一等",
        "三等",
        "上意下達",
        "上達",
        "下意上達",
        "下等",
        "不平等",
        "不等",
        "中等",
        "二等",
        "二親等",
        "伊達",
        "伝達",
        "何等",
        "優等",
        "先達",
        "初等",
        "到達",
        "劣等",
        "勲等",
        "即日速達",
        "友達",
        "同等",
        "品等",
        "四通八達",
        "均等",
        "女友達",
        "学校友達",
        "宮内庁御用達",
        "対等",
        "平等",
        "幼友達",
        "御用達",
        "悪平等",
        "数等",
        "新聞配達",
        "書留速達",
        "未発達",
        "栄達",
        "無料配達",
        "熟達",
        "牛乳配達",
        "特等",
        "男友達",
        "男女平等",
        "発達",
        "練達",
        "茶飲み友達",
        "親等",
        "調達",
        "送達",
        "通達",
        "速達",
        "遊び友達",
        "配達",
        "酒飲み友達",
        "闊達",
        "飲み友達",
        "高等",
    ];

    /// Reduplicated plurals, written with the iteration mark 々.
    pub(crate) static PLURAL_IRREGULAR: &[(&str, &str)] = &[
        ("人", "人々"),
        ("国", "国々"),
        ("山", "山々"),
        ("島", "島々"),
        ("年", "年々"),
        ("我", "我々"),
        ("所", "所々"),
        ("日", "日々"),
        ("星", "星々"),
        ("月", "月々"),
        ("神", "神々"),
        ("隅", "隅々"),
    ];

    /// The same pairs keyed by the plural, in both the 々 spelling and the
    /// fully written-out one, since both occur in running text.
    pub(crate) static SINGULAR_IRREGULAR: &[(&str, &str)] = &[
        ("人々", "人"),
        ("人人", "人"),
        ("国々", "国"),
        ("国国", "国"),
        ("山々", "山"),
        ("山山", "山"),
        ("島々", "島"),
        ("島島", "島"),
        ("年々", "年"),
        ("年年", "年"),
        ("我々", "我"),
        ("我我", "我"),
        ("所々", "所"),
        ("所所", "所"),
        ("日々", "日"),
        ("日日", "日"),
        ("星々", "星"),
        ("星星", "星"),
        ("月々", "月"),
        ("月月", "月"),
        ("神々", "神"),
        ("神神", "神"),
        ("隅々", "隅"),
        ("隅隅", "隅"),
    ];

    /// Suffixation with 〜たち, which is what "pluralising" a Japanese noun
    /// means here. No case restoration applies: Japanese script has no case.
    pub(crate) static PLURAL_REGULAR: &[RawRule] = &[rule("$", "たち", "私", "私たち")];

    /// Removal of a plural or associative suffix.
    pub(crate) static SINGULAR_REGULAR: &[RawRule] = &[
        rule("^(.+)たち$", "${1}", "人たち", "人"),
        rule("^(.+)達$", "${1}", "私達", "私"),
        rule("^(.+)等$", "${1}", "私等", "私"),
        rule(
            "^(人間|わたくし|私|てまえ|手前|野郎|やろう|勇者|がき|ガキ|餓鬼|あくとう|悪党|猫|家来)(?:共|ども)$",
            "${1}",
            "野郎共",
            "野郎",
        ),
        rule(
            "^(神様|先生|あなた|大名|女中|奥様)(?:方|がた)$",
            "${1}",
            "先生方",
            "先生",
        ),
    ];
}

/// English verbs, inflected for number agreement.
pub(crate) mod verb {
    use super::{RawRule, guard, rule};

    /// The modal auxiliaries, which have no third-person singular form.
    pub(crate) static INVARIANT: &[&str] = &["can", "may", "must", "ought", "shall", "will"];

    /// Third-person singular → plain form.
    ///
    /// `am` is first-person singular and `is` third; both pair with `are`.
    /// `was`/`were` are past-tense forms and are here because *be* is the only
    /// English verb that marks number outside the present tense.
    pub(crate) static PLURAL_IRREGULAR: &[(&str, &str)] = &[
        ("am", "are"),
        ("has", "have"),
        ("is", "are"),
        ("was", "were"),
    ];

    /// Plain form → third-person singular. `are` maps back to `is`, the
    /// third-person member of the pair, not to the first-person `am`.
    pub(crate) static SINGULAR_IRREGULAR: &[(&str, &str)] = &[
        ("are", "is"),
        // The plain present of *be* is `are`; `be` itself is the plain form of
        // the lexeme, and its third-person singular present is `is`.
        ("be", "is"),
        ("have", "has"),
        ("were", "was"),
    ];

    /// Third-person singular → plain form. First match wins.
    pub(crate) static PLURAL_REGULAR: &[RawRule] = &[
        rule("(?i)sses$", "ss", "passes", "pass"),
        rule("(?i)xes$", "x", "annexes", "annex"),
        rule("(?i)([cs])hes$", "${1}h", "catches", "catch"),
        rule("(?i)zzes$", "zz", "buzzes", "buzz"),
        rule("(?i)([^hzoi])es$", "${1}e", "makes", "make"),
        // The four verbs whose plain form ends in `-ie`.
        rule("(?i)^([dltv])ies$", "${1}ie", "dies", "die"),
        rule("(?i)ies$", "y", "flies", "fly"),
        rule("(?i)e?s$", "", "runs", "run"),
    ];

    /// Plain form → third-person singular. First match wins.
    pub(crate) static SINGULAR_REGULAR: &[RawRule] = &[
        // Forms that are already third-person singular. `am` is first person
        // and has no third-person singular of its own, so it is left as it is
        // rather than given one.
        guard("(?i)^(?:am|is|has|was)$", "am"),
        // Verbs whose plain form ends in `-ed`, which the guard below would
        // otherwise mistake for a past form.
        rule(
            "(?i)(bleed|breed|embed|exceed|feed|heed|lead|need|plead|proceed|read|seed|shed|shred|speed|spread|succeed|wed|weed)$",
            "${1}s",
            "need",
            "needs",
        ),
        // A past form has no third-person singular present, so it is left alone
        // rather than given one.
        guard("(?i)ed$", "watched"),
        rule("(?i)ss$", "sses", "pass", "passes"),
        rule("(?i)x$", "xes", "annex", "annexes"),
        rule("(?i)(h|z|o)$", "${1}es", "catch", "catches"),
        rule("(?i)([^aeiou])y$", "${1}ies", "fly", "flies"),
        rule("$", "s", "run", "runs"),
    ];
}
