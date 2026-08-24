//! The Lancaster (Paice/Husk) rule table.
//!
//! Checked-in data. **Array order is semantic**: the first rule whose pattern
//! matches and whose result is acceptable wins, so reordering this table
//! changes stems.
//!
//! # Grounded in the publication
//!
//! That order is no longer taken on trust. The published rule set — C. D.
//! Paice, "Another stemmer", *ACM SIGIR Forum* 24(3), 1990, distributed ever
//! since as a line-per-rule text file in Paice's own notation
//! (`re2>` = "remove 2 from `-er` and re-stem", `nee0.` = "protect `-een`",
//! `*` = intact-only) — is reproduced verbatim in `tests::PUBLISHED_RULE_SET`
//! and compared against these arrays entry by entry **and in order** by
//! `tests::the_table_is_the_published_paice_husk_rule_set`.
//!
//! It matches exactly: 115 rules, 21 sections, same patterns, same sizes, same
//! appendages, same continuation and intact flags, same sequence within every
//! section. The rule file's trailing `end0.` line is not a 116th rule; it is
//! the pseudo-rule that terminates rule loading, and the reference
//! implementations discard it by name rather than adding it to the set.
//!
//! # What the walk found
//!
//! Every rule was pushed through [`crate::lancaster::select_rule`], the engine's
//! own rule-choosing step, looking for an input that reaches it past the rules
//! that shadow it. **One rule of the 115 can never fire**: `rei3y>`
//! (`-ier > -y`), which the publication places after `re2>` (`-er > -`) in
//! section `r`. It is kept anyway, because this table's job is to be the
//! published table; `tests::the_ier_rule_is_dead_because_er_always_shadows_it`
//! records why it is dead and why deleting it would change no stem.

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
/// Paice/Husk rule table, so they short-circuit to "no rules apply".
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

#[cfg(test)]
mod tests {
    use super::{Rule, section};
    use crate::lancaster::{LancasterStemmer, select_rule};

    /// The published Paice/Husk rule set, verbatim, in publication order.
    ///
    /// # Provenance
    ///
    /// C. D. Paice, "Another stemmer", *ACM SIGIR Forum* 24(3), 1990, 56–61.
    /// The rule table has been distributed since as a text file, one rule per
    /// line, in the notation the paper defines. The lines below were taken from
    /// that file and cross-checked, character for character, against a second
    /// independent rendition — NLTK's `LancasterStemmer::default_rule_tuple`,
    /// which carries the same 115 rules in the same order with the same
    /// glosses. Two renditions that agree is the strongest grounding available
    /// without the paper's own typesetting; a single one would not have been
    /// enough to call this "the published set".
    ///
    /// # Notation
    ///
    /// `<ending><*?><size><appendage><flag>` where the ending is written
    /// **reversed** (`ai` is the suffix `-ia`), `*` marks an intact-only rule,
    /// `size` is a single decimal digit, and the flag is `>` to re-stem the
    /// result or `.` to stop.
    ///
    /// The rule file ends with a line `end0.`. It is not a rule — no English
    /// word ends `-dne` — it is the sentinel that terminates loading, and the
    /// reference implementations match it by name and stop before adding it.
    /// It is therefore absent here, exactly as it is absent from NLTK's tuple.
    const PUBLISHED_RULE_SET: &[&str] = &[
        "ai*2.",     // -ia > - if intact
        "a*1.",      // -a > - if intact
        "bb1.",      // -bb > -b
        "city3s.",   // -ytic > -ys
        "ci2>",      // -ic > -
        "cn1t>",     // -nc > -nt
        "dd1.",      // -dd > -d
        "dei3y>",    // -ied > -y
        "deec2ss.",  // -ceed > -cess
        "dee1.",     // -eed > -ee
        "de2>",      // -ed > -
        "dooh4>",    // -hood > -
        "e1>",       // -e > -
        "feil1v.",   // -lief > -liev
        "fi2>",      // -if > -
        "gni3>",     // -ing > -
        "gai3y.",    // -iag > -y
        "ga2>",      // -ag > -
        "gg1.",      // -gg > -g
        "ht*2.",     // -th > - if intact
        "hsiug5ct.", // -guish > -ct
        "hsi3>",     // -ish > -
        "i*1.",      // -i > - if intact
        "i1y>",      // -i > -y
        "ji1d.",     // -ij > -id -- see nois4j> & vis3j>
        "juf1s.",    // -fuj > -fus
        "ju1d.",     // -uj > -ud
        "jo1d.",     // -oj > -od
        "jeh1r.",    // -hej > -her
        "jrev1t.",   // -verj > -vert
        "jsim2t.",   // -misj > -mit
        "jn1d.",     // -nj > -nd
        "j1s.",      // -j > -s
        "lbaifi6.",  // -ifiabl > -
        "lbai4y.",   // -iabl > -y
        "lba3>",     // -abl > -
        "lbi3.",     // -ibl > -
        "lib2l>",    // -bil > -bl
        "lc1.",      // -cl > c
        "lufi4y.",   // -iful > -y
        "luf3>",     // -ful > -
        "lu2.",      // -ul > -
        "lai3>",     // -ial > -
        "lau3>",     // -ual > -
        "la2>",      // -al > -
        "ll1.",      // -ll > -l
        "mui3.",     // -ium > -
        "mu*2.",     // -um > - if intact
        "msi3>",     // -ism > -
        "mm1.",      // -mm > -m
        "nois4j>",   // -sion > -j
        "noix4ct.",  // -xion > -ct
        "noi3>",     // -ion > -
        "nai3>",     // -ian > -
        "na2>",      // -an > -
        "nee0.",     // protect -een
        "ne2>",      // -en > -
        "nn1.",      // -nn > -n
        "pihs4>",    // -ship > -
        "pp1.",      // -pp > -p
        "re2>",      // -er > -
        "rae0.",     // protect -ear
        "ra2.",      // -ar > -
        "ro2>",      // -or > -
        "ru2>",      // -ur > -
        "rr1.",      // -rr > -r
        "rt1>",      // -tr > -t
        "rei3y>",    // -ier > -y
        "sei3y>",    // -ies > -y
        "sis2.",     // -sis > -s
        "si2>",      // -is > -
        "ssen4>",    // -ness > -
        "ss0.",      // protect -ss
        "suo3>",     // -ous > -
        "su*2.",     // -us > - if intact
        "s*1>",      // -s > - if intact
        "s0.",       // -s > -s
        "tacilp4y.", // -plicat > -ply
        "ta2>",      // -at > -
        "tnem4>",    // -ment > -
        "tne3>",     // -ent > -
        "tna3>",     // -ant > -
        "tpir2b.",   // -ript > -rib
        "tpro2b.",   // -orpt > -orb
        "tcud1.",    // -duct > -duc
        "tpmus2.",   // -sumpt > -sum
        "tpec2iv.",  // -cept > -ceiv
        "tulo2v.",   // -olut > -olv
        "tsis0.",    // protect -sist
        "tsi3>",     // -ist > -
        "tt1.",      // -tt > -t
        "uqi3.",     // -iqu > -
        "ugo1.",     // -ogu > -og
        "vis3j>",    // -siv > -j
        "vie0.",     // protect -eiv
        "vi2>",      // -iv > -
        "ylb1>",     // -bly > -bl
        "yli3y>",    // -ily > -y
        "ylp0.",     // protect -ply
        "yl2>",      // -ly > -
        "ygo1.",     // -ogy > -og
        "yhp1.",     // -phy > -ph
        "ymo1.",     // -omy > -om
        "ypo1.",     // -opy > -op
        "yti3>",     // -ity > -
        "yte3>",     // -ety > -
        "ytl2.",     // -lty > -l
        "yrtsi5.",   // -istry > -
        "yra3>",     // -ary > -
        "yro3>",     // -ory > -
        "yfi3.",     // -ify > -
        "ycn2t>",    // -ncy > -nt
        "yca3>",     // -acy > -
        "zi2>",      // -iz > -
        "zy1s.",     // -yz > -ys
    ];

    /// One rule of `PUBLISHED_RULE_SET`, decoded into the shape this table
    /// stores.
    #[derive(Debug, PartialEq, Eq)]
    struct Decoded {
        section: char,
        pattern: String,
        size: usize,
        appendage: Option<String>,
        continuation: bool,
        intact: bool,
    }

    impl Decoded {
        /// Parses one line of Paice's notation. Deliberately strict: a line the
        /// parser cannot account for in full is a panic, not a skip, so a
        /// mistyped rule cannot quietly drop out of the comparison.
        fn parse(rule: &str) -> Self {
            let bytes = rule.as_bytes();
            let mut i = 0;
            while i < bytes.len() && bytes[i].is_ascii_lowercase() {
                i += 1;
            }
            let ending = &rule[..i];
            assert!(!ending.is_empty(), "{rule:?}: no ending");
            let intact = bytes.get(i) == Some(&b'*');
            i += usize::from(intact);
            let size = match bytes.get(i) {
                Some(d) if d.is_ascii_digit() => usize::from(d - b'0'),
                _ => panic!("{rule:?}: no size digit"),
            };
            i += 1;
            let appendage_start = i;
            while i < bytes.len() && bytes[i].is_ascii_lowercase() {
                i += 1;
            }
            let appendage = &rule[appendage_start..i];
            let continuation = match bytes.get(i) {
                Some(b'>') => true,
                Some(b'.') => false,
                _ => panic!("{rule:?}: no continue/stop flag"),
            };
            assert_eq!(i + 1, bytes.len(), "{rule:?}: trailing text after the flag");
            Self {
                // The ending is written reversed, so its first character is the
                // suffix's last one — which is exactly the section key.
                section: ending.chars().next().expect("non-empty"),
                pattern: ending.chars().rev().collect(),
                size,
                appendage: (!appendage.is_empty()).then(|| appendage.to_owned()),
                continuation,
                intact,
            }
        }

        fn from_table(section: char, rule: &Rule) -> Self {
            Self {
                section,
                pattern: rule.pattern.to_owned(),
                size: rule.size,
                appendage: rule.appendage.map(str::to_owned),
                continuation: rule.continuation,
                intact: rule.intact,
            }
        }
    }

    /// This table **is** the published Paice/Husk rule set — same rules, same
    /// order, section by section.
    ///
    /// This is the check the file's own doc comment used to say was owed. The
    /// order is what makes it matter: the engine takes the first rule whose
    /// result is acceptable, so two tables holding the same 115 rules in
    /// different sequences are two different stemmers. Comparing sets would
    /// have passed on a reordered table. Not every reorder moves a stem — swapping
    /// `-er` and `-ier` inside section `r` changes none, because the two reach the
    /// same state — but a comparison that cannot see order cannot tell which.
    ///
    /// Sections are compared as whole sequences rather than rule-by-rule with a
    /// zip, so an insertion or a deletion shows up as the length mismatch it is
    /// instead of as a cascade of unrelated inequalities.
    #[test]
    fn the_table_is_the_published_paice_husk_rule_set() {
        assert_eq!(
            PUBLISHED_RULE_SET.len(),
            115,
            "the transcription of the published rule set changed length"
        );

        // Group the publication by section, keeping publication order. The rule
        // file is already contiguous per section, which is itself worth
        // asserting: a section that reopened later would mean the file order
        // and the per-section order are not the same thing.
        let mut published: Vec<(char, Vec<Decoded>)> = Vec::new();
        for line in PUBLISHED_RULE_SET {
            let decoded = Decoded::parse(line);
            match published.last_mut() {
                Some((section, rules)) if *section == decoded.section => rules.push(decoded),
                _ => {
                    assert!(
                        !published.iter().any(|(s, _)| *s == decoded.section),
                        "section {:?} reopens after another section in the rule file",
                        decoded.section
                    );
                    published.push((decoded.section, vec![decoded]));
                }
            }
        }
        assert_eq!(
            published.iter().map(|(s, _)| *s).collect::<String>(),
            "abcdefghijlmnprstuvyz",
            "the set of sections in the published rule set changed"
        );

        let mut audited = 0;
        for (letter, rules) in &published {
            let ours: Vec<Decoded> = section(*letter)
                .iter()
                .map(|rule| Decoded::from_table(*letter, rule))
                .collect();
            assert_eq!(
                &ours, rules,
                "section {letter:?} does not match the published rule set"
            );
            audited += rules.len();
        }
        assert_eq!(audited, 115, "not every published rule was compared");

        // ...and nothing outside the publication is dispatchable. `k o q w x`
        // have no rules in Paice's table, and `section` must agree.
        for last in 'a'..='z' {
            let has_rules = published.iter().any(|(s, _)| *s == last);
            assert_eq!(
                !section(last).is_empty(),
                has_rules,
                "section {last:?} disagrees with the publication about existing"
            );
        }
        assert!(
            "koqwx".chars().all(|c| section(c).is_empty()),
            "a section the publication does not define acquired rules"
        );
    }

    /// Candidate prefixes for the reachability search: every string of up to
    /// three lower-case letters.
    ///
    /// Three is enough by construction rather than by luck, from both
    /// directions:
    ///
    /// * **Long enough for the target.** A rule's candidate is at worst
    ///   `prefix + appendage` — the whole matched suffix removed and nothing
    ///   put back — and `acceptable` never asks for more than three units, so
    ///   three letters already clear its longest bar. Every three-letter
    ///   combination is tried, so a vowel-bearing one is always among them.
    /// * **Short enough not to invite shadowing.** Lengthening a prefix can
    ///   only *add* ways for an earlier rule to match and to be acceptable, so
    ///   a rule that no prefix of length ≤ 3 reaches is not going to be reached
    ///   by a longer one.
    fn search_prefixes() -> Vec<String> {
        let mut out = vec![String::new()];
        for a in 'a'..='z' {
            out.push(a.to_string());
            for b in 'a'..='z' {
                out.push(format!("{a}{b}"));
                for c in 'a'..='z' {
                    out.push(format!("{a}{b}{c}"));
                }
            }
        }
        out
    }

    /// Every rule but one has an input that actually reaches it.
    ///
    /// This is the walk this table had never had. Sitting in the right section
    /// with a well-formed `size` — which
    /// `data::table_audit::every_lancaster_rule_is_reachable_and_cuts_only_what_it_matched`
    /// already checks — is not the same as being reachable: earlier rules in
    /// the same section shadow later ones whenever they match *and* their
    /// result is acceptable, and a rule nothing can reach is dead weight at
    /// best and a transcription error at worst.
    ///
    /// So each rule is asked for a witness: an input on which
    /// [`crate::lancaster::select_rule`] — the engine's own choosing step, not a re-derivation
    /// beside it — returns *that* rule. Identity is by pointer, because two
    /// rules in this table share a pattern (`i` and `s`, each once) and only
    /// the address distinguishes them.
    ///
    /// Witnesses are sought with the token intact, which is the hardest case:
    /// at the first step every earlier rule is live, including the intact-only
    /// ones that a later step would disable. A rule with a witness there is
    /// reachable from a real call to `stem`.
    #[test]
    fn every_rule_but_one_has_an_input_that_reaches_it() {
        let prefixes = search_prefixes();
        let mut audited = 0;
        let mut unreachable: Vec<String> = Vec::new();
        for letter in 'a'..='z' {
            let rules = section(letter);
            for (index, rule) in rules.iter().enumerate() {
                audited += 1;
                let reached = prefixes.iter().any(|prefix| {
                    let token = format!("{prefix}{}", rule.pattern);
                    matches!(select_rule(&token, true), Some((won, _)) if std::ptr::eq(won, rule))
                });
                if !reached {
                    unreachable.push(format!("{letter}[{index}] -{}", rule.pattern));
                }
            }
        }
        assert_eq!(audited, 115, "the number of rules walked changed");
        assert_eq!(
            unreachable,
            ["r[7] -ier"],
            "the set of Lancaster rules no input can reach changed"
        );
    }

    /// Why `-ier > -y` is dead, and why that is the publication's doing.
    ///
    /// `re2>` (`-er > -`) is the first rule of section `r` and `rei3y>`
    /// (`-ier > -y`) is the last. Every token ending `-ier` also ends `-er`, so
    /// the two compete on every input the later rule could ever match — and
    /// they are accepted or rejected *together*. For `X + "ier"` the candidates
    /// are `X + "i"` and `X + "y"`: same length, same first character, and each
    /// holds a vowel-or-`y` of its own, so every arm of `acceptable` returns the
    /// same verdict for both. `-er` is first, therefore `-er` always wins.
    /// (`X` empty is the one case where the first characters differ, `"i"`
    /// against `"y"`; both are one unit long and both are rejected.)
    ///
    /// Nothing is lost. `-er` re-stems, `X + "i"` lands in section `i`, and
    /// `i1y>` rewrites it to `X + "y"` and re-stems again — the exact state
    /// `-ier > -y` would have produced. The rule is redundant, not just
    /// unreachable, which is why it stays: removing it would make this table
    /// something other than the published one and would still change no stem.
    #[test]
    fn the_ier_rule_is_dead_because_er_always_shadows_it() {
        let r = section('r');
        assert_eq!(r[0].pattern, "er", "-er is no longer first in section r");
        assert_eq!(r[7].pattern, "ier", "-ier is no longer last in section r");
        assert_eq!(r.len(), 8);

        let mut probes = search_prefixes();
        // Non-ASCII prefixes too: `acceptable` reads the first *byte* for its
        // vowel test and counts scalar values for length, and the argument above
        // has to survive both.
        probes
            .extend(["café", "naïv", "ünï", "日本", "\u{1F600}", &"x".repeat(30)].map(Into::into));

        let stemmer = LancasterStemmer::new();
        let mut shadowed = 0;
        let mut inert = 0;
        for prefix in &probes {
            let token = format!("{prefix}ier");
            match select_rule(&token, true) {
                Some((won, _)) => {
                    assert!(
                        std::ptr::eq(won, &r[0]),
                        "{token:?} reached -{} rather than -er",
                        won.pattern
                    );
                    shadowed += 1;
                    // ...and the `-er` path ends exactly where `-ier > -y`
                    // would have: at the stem of `X + "y"`.
                    assert_eq!(
                        stemmer.stem(&token),
                        stemmer.stem(&format!("{prefix}y")),
                        "the -er path diverged from what -ier > -y would give for {token:?}"
                    );
                }
                None => {
                    // Neither candidate is acceptable, so the token is returned
                    // whole — `-ier` does not get a turn here either.
                    inert += 1;
                    assert_eq!(stemmer.stem(&token), token);
                }
            }
        }
        assert_eq!(shadowed + inert, probes.len());
        assert!(shadowed > 0 && inert > 0, "both branches must be exercised");
    }

    /// The seven size-0 rules, and that they are the publication's protects.
    ///
    /// `crate::lancaster`'s module doc used to say six. There are seven: the
    /// publication's six `protect` rules plus `s0.`, `{ -s > -s }`, which
    /// removes nothing and appends nothing and so protects as surely as the
    /// ones written that way. The count is load-bearing — a size-0 rule returns
    /// the token verbatim and blocks every later rule in its section, so
    /// gaining or losing one changes stems — and it is pinned here rather than
    /// left as prose.
    #[test]
    fn the_size_zero_rules_are_the_seven_protect_rules() {
        let mut protects: Vec<String> = Vec::new();
        for letter in 'a'..='z' {
            for rule in section(letter) {
                if rule.size == 0 {
                    assert!(
                        !rule.continuation,
                        "-{} returns its input, so re-stemming it would not terminate",
                        rule.pattern
                    );
                    assert_eq!(
                        rule.appendage, None,
                        "-{} appends while removing nothing, which is not a protect",
                        rule.pattern
                    );
                    protects.push(format!("-{}", rule.pattern));
                }
            }
        }
        assert_eq!(
            protects,
            ["-een", "-ear", "-ss", "-s", "-sist", "-eiv", "-ply"],
            "the set of size-0 protect rules changed"
        );
    }

    /// Two patterns appear twice, and only the intact flag tells them apart.
    ///
    /// `i*1.` / `i1y>` and `s*1>` / `s0.` are the publication's, not a
    /// duplication accident: in each pair the intact-only rule comes first and
    /// the general one follows, which is the only order in which both can fire.
    /// Swap them and the intact rule becomes unreachable, exactly the way
    /// `-ier` is — so the ordering is pinned, and both members of both pairs are
    /// required to be reachable.
    #[test]
    fn the_two_repeated_patterns_are_ordered_intact_first() {
        let mut repeated: Vec<String> = Vec::new();
        for letter in 'a'..='z' {
            let rules = section(letter);
            for (index, rule) in rules.iter().enumerate() {
                let twin = rules
                    .iter()
                    .enumerate()
                    .find(|(other, r)| *other != index && r.pattern == rule.pattern);
                let Some((other, twin)) = twin else { continue };
                if other < index {
                    continue;
                }
                assert!(
                    rule.intact && !twin.intact,
                    "section {letter:?}: -{} is listed twice without the intact flag \
                     separating the two, so one copy can never fire",
                    rule.pattern
                );
                repeated.push(format!("{letter}[{index},{other}] -{}", rule.pattern));
            }
        }
        assert_eq!(
            repeated,
            ["i[0,1] -i", "s[7,8] -s"],
            "the set of patterns listed twice changed"
        );
    }
}
