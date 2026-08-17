//! The Lancaster (Paice/Husk) rule table, transcribed from
//! The reference `lancaster_rules`.
//!
//! Generated data. Array order is semantic — the first rule whose pattern matches
//! *and* whose result is acceptable wins — so the order here is byte-for-byte the
//! order in the reference. `size` is a decimal STRING in the reference; it is parsed
//! once, here, into a `u8`.

use crate::lancaster::Rule;

/// Rules for words ending in `a`.
static SECTION_A: &[Rule] = &[
    Rule {
        pattern: "ia",
        size: 2,
        appendage: None,
        continuation: false,
        intact: true,
    },
    Rule {
        pattern: "a",
        size: 1,
        appendage: None,
        continuation: false,
        intact: true,
    },
];

/// Rules for words ending in `b`.
static SECTION_B: &[Rule] = &[Rule {
    pattern: "bb",
    size: 1,
    appendage: None,
    continuation: false,
    intact: false,
}];

/// Rules for words ending in `c`.
static SECTION_C: &[Rule] = &[
    Rule {
        pattern: "ytic",
        size: 3,
        appendage: Some("s"),
        continuation: false,
        intact: false,
    },
    Rule {
        pattern: "ic",
        size: 2,
        appendage: None,
        continuation: true,
        intact: false,
    },
    Rule {
        pattern: "nc",
        size: 1,
        appendage: Some("t"),
        continuation: true,
        intact: false,
    },
];

/// Rules for words ending in `d`.
static SECTION_D: &[Rule] = &[
    Rule {
        pattern: "dd",
        size: 1,
        appendage: None,
        continuation: false,
        intact: false,
    },
    Rule {
        pattern: "ied",
        size: 3,
        appendage: Some("y"),
        continuation: true,
        intact: false,
    },
    Rule {
        pattern: "ceed",
        size: 2,
        appendage: Some("ss"),
        continuation: false,
        intact: false,
    },
    Rule {
        pattern: "eed",
        size: 1,
        appendage: None,
        continuation: false,
        intact: false,
    },
    Rule {
        pattern: "ed",
        size: 2,
        appendage: None,
        continuation: true,
        intact: false,
    },
    Rule {
        pattern: "hood",
        size: 4,
        appendage: None,
        continuation: true,
        intact: false,
    },
];

/// Rules for words ending in `e`.
static SECTION_E: &[Rule] = &[Rule {
    pattern: "e",
    size: 1,
    appendage: None,
    continuation: true,
    intact: false,
}];

/// Rules for words ending in `f`.
static SECTION_F: &[Rule] = &[
    Rule {
        pattern: "lief",
        size: 1,
        appendage: Some("v"),
        continuation: false,
        intact: false,
    },
    Rule {
        pattern: "if",
        size: 2,
        appendage: None,
        continuation: true,
        intact: false,
    },
];

/// Rules for words ending in `g`.
static SECTION_G: &[Rule] = &[
    Rule {
        pattern: "ing",
        size: 3,
        appendage: None,
        continuation: true,
        intact: false,
    },
    Rule {
        pattern: "iag",
        size: 3,
        appendage: Some("y"),
        continuation: false,
        intact: false,
    },
    Rule {
        pattern: "ag",
        size: 2,
        appendage: None,
        continuation: true,
        intact: false,
    },
    Rule {
        pattern: "gg",
        size: 1,
        appendage: None,
        continuation: false,
        intact: false,
    },
];

/// Rules for words ending in `h`.
static SECTION_H: &[Rule] = &[
    Rule {
        pattern: "th",
        size: 2,
        appendage: None,
        continuation: false,
        intact: true,
    },
    Rule {
        pattern: "guish",
        size: 5,
        appendage: Some("ct"),
        continuation: false,
        intact: false,
    },
    Rule {
        pattern: "ish",
        size: 3,
        appendage: None,
        continuation: true,
        intact: false,
    },
];

/// Rules for words ending in `i`.
static SECTION_I: &[Rule] = &[
    Rule {
        pattern: "i",
        size: 1,
        appendage: None,
        continuation: false,
        intact: true,
    },
    Rule {
        pattern: "i",
        size: 1,
        appendage: Some("y"),
        continuation: true,
        intact: false,
    },
];

/// Rules for words ending in `j`.
static SECTION_J: &[Rule] = &[
    Rule {
        pattern: "ij",
        size: 1,
        appendage: Some("d"),
        continuation: false,
        intact: false,
    },
    Rule {
        pattern: "fuj",
        size: 1,
        appendage: Some("s"),
        continuation: false,
        intact: false,
    },
    Rule {
        pattern: "uj",
        size: 1,
        appendage: Some("d"),
        continuation: false,
        intact: false,
    },
    Rule {
        pattern: "oj",
        size: 1,
        appendage: Some("d"),
        continuation: false,
        intact: false,
    },
    Rule {
        pattern: "hej",
        size: 1,
        appendage: Some("r"),
        continuation: false,
        intact: false,
    },
    Rule {
        pattern: "verj",
        size: 1,
        appendage: Some("t"),
        continuation: false,
        intact: false,
    },
    Rule {
        pattern: "misj",
        size: 2,
        appendage: Some("t"),
        continuation: false,
        intact: false,
    },
    Rule {
        pattern: "nj",
        size: 1,
        appendage: Some("d"),
        continuation: false,
        intact: false,
    },
    Rule {
        pattern: "j",
        size: 1,
        appendage: Some("s"),
        continuation: false,
        intact: false,
    },
];

/// Rules for words ending in `l`.
static SECTION_L: &[Rule] = &[
    Rule {
        pattern: "ifiabl",
        size: 6,
        appendage: None,
        continuation: false,
        intact: false,
    },
    Rule {
        pattern: "iabl",
        size: 4,
        appendage: Some("y"),
        continuation: false,
        intact: false,
    },
    Rule {
        pattern: "abl",
        size: 3,
        appendage: None,
        continuation: true,
        intact: false,
    },
    Rule {
        pattern: "ibl",
        size: 3,
        appendage: None,
        continuation: false,
        intact: false,
    },
    Rule {
        pattern: "bil",
        size: 2,
        appendage: Some("l"),
        continuation: true,
        intact: false,
    },
    Rule {
        pattern: "cl",
        size: 1,
        appendage: None,
        continuation: false,
        intact: false,
    },
    Rule {
        pattern: "iful",
        size: 4,
        appendage: Some("y"),
        continuation: false,
        intact: false,
    },
    Rule {
        pattern: "ful",
        size: 3,
        appendage: None,
        continuation: true,
        intact: false,
    },
    Rule {
        pattern: "ul",
        size: 2,
        appendage: None,
        continuation: false,
        intact: false,
    },
    Rule {
        pattern: "ial",
        size: 3,
        appendage: None,
        continuation: true,
        intact: false,
    },
    Rule {
        pattern: "ual",
        size: 3,
        appendage: None,
        continuation: true,
        intact: false,
    },
    Rule {
        pattern: "al",
        size: 2,
        appendage: None,
        continuation: true,
        intact: false,
    },
    Rule {
        pattern: "ll",
        size: 1,
        appendage: None,
        continuation: false,
        intact: false,
    },
];

/// Rules for words ending in `m`.
static SECTION_M: &[Rule] = &[
    Rule {
        pattern: "ium",
        size: 3,
        appendage: None,
        continuation: false,
        intact: false,
    },
    Rule {
        pattern: "um",
        size: 2,
        appendage: None,
        continuation: false,
        intact: true,
    },
    Rule {
        pattern: "ism",
        size: 3,
        appendage: None,
        continuation: true,
        intact: false,
    },
    Rule {
        pattern: "mm",
        size: 1,
        appendage: None,
        continuation: false,
        intact: false,
    },
];

/// Rules for words ending in `n`.
static SECTION_N: &[Rule] = &[
    Rule {
        pattern: "sion",
        size: 4,
        appendage: Some("j"),
        continuation: true,
        intact: false,
    },
    Rule {
        pattern: "xion",
        size: 4,
        appendage: Some("ct"),
        continuation: false,
        intact: false,
    },
    Rule {
        pattern: "ion",
        size: 3,
        appendage: None,
        continuation: true,
        intact: false,
    },
    Rule {
        pattern: "ian",
        size: 3,
        appendage: None,
        continuation: true,
        intact: false,
    },
    Rule {
        pattern: "an",
        size: 2,
        appendage: None,
        continuation: true,
        intact: false,
    },
    Rule {
        pattern: "een",
        size: 0,
        appendage: None,
        continuation: false,
        intact: false,
    },
    Rule {
        pattern: "en",
        size: 2,
        appendage: None,
        continuation: true,
        intact: false,
    },
    Rule {
        pattern: "nn",
        size: 1,
        appendage: None,
        continuation: false,
        intact: false,
    },
];

/// Rules for words ending in `p`.
static SECTION_P: &[Rule] = &[
    Rule {
        pattern: "ship",
        size: 4,
        appendage: None,
        continuation: true,
        intact: false,
    },
    Rule {
        pattern: "pp",
        size: 1,
        appendage: None,
        continuation: false,
        intact: false,
    },
];

/// Rules for words ending in `r`.
static SECTION_R: &[Rule] = &[
    Rule {
        pattern: "er",
        size: 2,
        appendage: None,
        continuation: true,
        intact: false,
    },
    Rule {
        pattern: "ear",
        size: 0,
        appendage: None,
        continuation: false,
        intact: false,
    },
    Rule {
        pattern: "ar",
        size: 2,
        appendage: None,
        continuation: false,
        intact: false,
    },
    Rule {
        pattern: "or",
        size: 2,
        appendage: None,
        continuation: true,
        intact: false,
    },
    Rule {
        pattern: "ur",
        size: 2,
        appendage: None,
        continuation: true,
        intact: false,
    },
    Rule {
        pattern: "rr",
        size: 1,
        appendage: None,
        continuation: false,
        intact: false,
    },
    Rule {
        pattern: "tr",
        size: 1,
        appendage: None,
        continuation: true,
        intact: false,
    },
    Rule {
        pattern: "ier",
        size: 3,
        appendage: Some("y"),
        continuation: true,
        intact: false,
    },
];

/// Rules for words ending in `s`.
static SECTION_S: &[Rule] = &[
    Rule {
        pattern: "ies",
        size: 3,
        appendage: Some("y"),
        continuation: true,
        intact: false,
    },
    Rule {
        pattern: "sis",
        size: 2,
        appendage: None,
        continuation: false,
        intact: false,
    },
    Rule {
        pattern: "is",
        size: 2,
        appendage: None,
        continuation: true,
        intact: false,
    },
    Rule {
        pattern: "ness",
        size: 4,
        appendage: None,
        continuation: true,
        intact: false,
    },
    Rule {
        pattern: "ss",
        size: 0,
        appendage: None,
        continuation: false,
        intact: false,
    },
    Rule {
        pattern: "ous",
        size: 3,
        appendage: None,
        continuation: true,
        intact: false,
    },
    Rule {
        pattern: "us",
        size: 2,
        appendage: None,
        continuation: false,
        intact: true,
    },
    Rule {
        pattern: "s",
        size: 1,
        appendage: None,
        continuation: true,
        intact: true,
    },
    Rule {
        pattern: "s",
        size: 0,
        appendage: None,
        continuation: false,
        intact: false,
    },
];

/// Rules for words ending in `t`.
static SECTION_T: &[Rule] = &[
    Rule {
        pattern: "plicat",
        size: 4,
        appendage: Some("y"),
        continuation: false,
        intact: false,
    },
    Rule {
        pattern: "at",
        size: 2,
        appendage: None,
        continuation: true,
        intact: false,
    },
    Rule {
        pattern: "ment",
        size: 4,
        appendage: None,
        continuation: true,
        intact: false,
    },
    Rule {
        pattern: "ent",
        size: 3,
        appendage: None,
        continuation: true,
        intact: false,
    },
    Rule {
        pattern: "ant",
        size: 3,
        appendage: None,
        continuation: true,
        intact: false,
    },
    Rule {
        pattern: "ript",
        size: 2,
        appendage: Some("b"),
        continuation: false,
        intact: false,
    },
    Rule {
        pattern: "orpt",
        size: 2,
        appendage: Some("b"),
        continuation: false,
        intact: false,
    },
    Rule {
        pattern: "duct",
        size: 1,
        appendage: None,
        continuation: false,
        intact: false,
    },
    Rule {
        pattern: "sumpt",
        size: 2,
        appendage: None,
        continuation: false,
        intact: false,
    },
    Rule {
        pattern: "cept",
        size: 2,
        appendage: Some("iv"),
        continuation: false,
        intact: false,
    },
    Rule {
        pattern: "olut",
        size: 2,
        appendage: Some("v"),
        continuation: false,
        intact: false,
    },
    Rule {
        pattern: "sist",
        size: 0,
        appendage: None,
        continuation: false,
        intact: false,
    },
    Rule {
        pattern: "ist",
        size: 3,
        appendage: None,
        continuation: true,
        intact: false,
    },
    Rule {
        pattern: "tt",
        size: 1,
        appendage: None,
        continuation: false,
        intact: false,
    },
];

/// Rules for words ending in `u`.
static SECTION_U: &[Rule] = &[
    Rule {
        pattern: "iqu",
        size: 3,
        appendage: None,
        continuation: false,
        intact: false,
    },
    Rule {
        pattern: "ogu",
        size: 1,
        appendage: None,
        continuation: false,
        intact: false,
    },
];

/// Rules for words ending in `v`.
static SECTION_V: &[Rule] = &[
    Rule {
        pattern: "siv",
        size: 3,
        appendage: Some("j"),
        continuation: true,
        intact: false,
    },
    Rule {
        pattern: "eiv",
        size: 0,
        appendage: None,
        continuation: false,
        intact: false,
    },
    Rule {
        pattern: "iv",
        size: 2,
        appendage: None,
        continuation: true,
        intact: false,
    },
];

/// Rules for words ending in `y`.
static SECTION_Y: &[Rule] = &[
    Rule {
        pattern: "bly",
        size: 1,
        appendage: None,
        continuation: true,
        intact: false,
    },
    Rule {
        pattern: "ily",
        size: 3,
        appendage: Some("y"),
        continuation: true,
        intact: false,
    },
    Rule {
        pattern: "ply",
        size: 0,
        appendage: None,
        continuation: false,
        intact: false,
    },
    Rule {
        pattern: "ly",
        size: 2,
        appendage: None,
        continuation: true,
        intact: false,
    },
    Rule {
        pattern: "ogy",
        size: 1,
        appendage: None,
        continuation: false,
        intact: false,
    },
    Rule {
        pattern: "phy",
        size: 1,
        appendage: None,
        continuation: false,
        intact: false,
    },
    Rule {
        pattern: "omy",
        size: 1,
        appendage: None,
        continuation: false,
        intact: false,
    },
    Rule {
        pattern: "opy",
        size: 1,
        appendage: None,
        continuation: false,
        intact: false,
    },
    Rule {
        pattern: "ity",
        size: 3,
        appendage: None,
        continuation: true,
        intact: false,
    },
    Rule {
        pattern: "ety",
        size: 3,
        appendage: None,
        continuation: true,
        intact: false,
    },
    Rule {
        pattern: "lty",
        size: 2,
        appendage: None,
        continuation: false,
        intact: false,
    },
    Rule {
        pattern: "istry",
        size: 5,
        appendage: None,
        continuation: false,
        intact: false,
    },
    Rule {
        pattern: "ary",
        size: 3,
        appendage: None,
        continuation: true,
        intact: false,
    },
    Rule {
        pattern: "ory",
        size: 3,
        appendage: None,
        continuation: true,
        intact: false,
    },
    Rule {
        pattern: "ify",
        size: 3,
        appendage: None,
        continuation: false,
        intact: false,
    },
    Rule {
        pattern: "ncy",
        size: 2,
        appendage: Some("t"),
        continuation: true,
        intact: false,
    },
    Rule {
        pattern: "acy",
        size: 3,
        appendage: None,
        continuation: true,
        intact: false,
    },
];

/// Rules for words ending in `z`.
static SECTION_Z: &[Rule] = &[
    Rule {
        pattern: "iz",
        size: 2,
        appendage: None,
        continuation: true,
        intact: false,
    },
    Rule {
        pattern: "yz",
        size: 1,
        appendage: Some("s"),
        continuation: false,
        intact: false,
    },
];

/// The rule section for a word's final character, or an empty slice.
///
/// Sections `k o q w x` and every non-`[a-z]` character are absent from the
/// reference table, so they short-circuit to "no rules apply".
pub(crate) fn section(last: char) -> &'static [Rule] {
    match last {
        'a' => SECTION_A,
        'b' => SECTION_B,
        'c' => SECTION_C,
        'd' => SECTION_D,
        'e' => SECTION_E,
        'f' => SECTION_F,
        'g' => SECTION_G,
        'h' => SECTION_H,
        'i' => SECTION_I,
        'j' => SECTION_J,
        'l' => SECTION_L,
        'm' => SECTION_M,
        'n' => SECTION_N,
        'p' => SECTION_P,
        'r' => SECTION_R,
        's' => SECTION_S,
        't' => SECTION_T,
        'u' => SECTION_U,
        'v' => SECTION_V,
        'y' => SECTION_Y,
        'z' => SECTION_Z,
        _ => &[],
    }
}
