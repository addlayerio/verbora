//! Tables for the Japanese transliterator, machine-derived from the reference.
//!
//! DO NOT EDIT BY HAND. These were produced by loading the reference module and
//! dumping what it actually built, plus parsing its 30 `(?=[...])` lookahead
//! rules out of the source text — never by transcription, so a change to either
//! could not silently fail to reach this port.
//!
//! On every run the generator re-proves the three properties the scanners rely
//! on:
//!
//! 1. **No key is a proper prefix of a later key**, in any of the three tables.
//!    That is what makes the leftmost-*first* alternation `replacer()` compiles
//!    identical to the leftmost-*longest* matching [`Table`] implements — and,
//!    unlike leftmost-first, leftmost-longest does not depend on the reference
//!    object iteration order surviving the port.
//! 2. **The 30 sequential lookahead passes fuse into one scan.** Not a general
//!    property of lookahead rewriting: it holds because every replacement is
//!    ASCII (so a rewritten position is inert for every later pass) and because
//!    the only source characters that also appear in a lookahead class — ン and
//!    ん — are read by rules that run before any rule that could consume them.
//! 3. **The whole model reproduces `TransliterateJa`**, over 120,000 random
//!    strings plus every ordered pair of kana.

use crate::scan::{Slot, Table, Window};

/// Phase 1 (`replace1`): the u/vu digraphs, all twenty of them two `char`s long.
///
/// 20 entries: 0 one-`char`, 20 two-`char`, 0 three-`char`.
pub(crate) static TABLE1: Table = Table {
    windows: &[Window {
        base: 0x3046,
        slots: &[
            Slot {
                one: None,
                two: (0, 5),
                three: (0, 0),
            }, // う
            Slot {
                one: None,
                two: (0, 0),
                three: (0, 0),
            },
            Slot {
                one: None,
                two: (0, 0),
                three: (0, 0),
            },
            Slot {
                one: None,
                two: (0, 0),
                three: (0, 0),
            },
            Slot {
                one: None,
                two: (0, 0),
                three: (0, 0),
            },
            Slot {
                one: None,
                two: (0, 0),
                three: (0, 0),
            },
            Slot {
                one: None,
                two: (0, 0),
                three: (0, 0),
            },
            Slot {
                one: None,
                two: (0, 0),
                three: (0, 0),
            },
            Slot {
                one: None,
                two: (0, 0),
                three: (0, 0),
            },
            Slot {
                one: None,
                two: (0, 0),
                three: (0, 0),
            },
            Slot {
                one: None,
                two: (0, 0),
                three: (0, 0),
            },
            Slot {
                one: None,
                two: (0, 0),
                three: (0, 0),
            },
            Slot {
                one: None,
                two: (0, 0),
                three: (0, 0),
            },
            Slot {
                one: None,
                two: (0, 0),
                three: (0, 0),
            },
            Slot {
                one: None,
                two: (0, 0),
                three: (0, 0),
            },
            Slot {
                one: None,
                two: (0, 0),
                three: (0, 0),
            },
            Slot {
                one: None,
                two: (0, 0),
                three: (0, 0),
            },
            Slot {
                one: None,
                two: (0, 0),
                three: (0, 0),
            },
            Slot {
                one: None,
                two: (0, 0),
                three: (0, 0),
            },
            Slot {
                one: None,
                two: (0, 0),
                three: (0, 0),
            },
            Slot {
                one: None,
                two: (0, 0),
                three: (0, 0),
            },
            Slot {
                one: None,
                two: (0, 0),
                three: (0, 0),
            },
            Slot {
                one: None,
                two: (0, 0),
                three: (0, 0),
            },
            Slot {
                one: None,
                two: (0, 0),
                three: (0, 0),
            },
            Slot {
                one: None,
                two: (0, 0),
                three: (0, 0),
            },
            Slot {
                one: None,
                two: (0, 0),
                three: (0, 0),
            },
            Slot {
                one: None,
                two: (0, 0),
                three: (0, 0),
            },
            Slot {
                one: None,
                two: (0, 0),
                three: (0, 0),
            },
            Slot {
                one: None,
                two: (0, 0),
                three: (0, 0),
            },
            Slot {
                one: None,
                two: (0, 0),
                three: (0, 0),
            },
            Slot {
                one: None,
                two: (0, 0),
                three: (0, 0),
            },
            Slot {
                one: None,
                two: (0, 0),
                three: (0, 0),
            },
            Slot {
                one: None,
                two: (0, 0),
                three: (0, 0),
            },
            Slot {
                one: None,
                two: (0, 0),
                three: (0, 0),
            },
            Slot {
                one: None,
                two: (0, 0),
                three: (0, 0),
            },
            Slot {
                one: None,
                two: (0, 0),
                three: (0, 0),
            },
            Slot {
                one: None,
                two: (0, 0),
                three: (0, 0),
            },
            Slot {
                one: None,
                two: (0, 0),
                three: (0, 0),
            },
            Slot {
                one: None,
                two: (0, 0),
                three: (0, 0),
            },
            Slot {
                one: None,
                two: (0, 0),
                three: (0, 0),
            },
            Slot {
                one: None,
                two: (0, 0),
                three: (0, 0),
            },
            Slot {
                one: None,
                two: (0, 0),
                three: (0, 0),
            },
            Slot {
                one: None,
                two: (0, 0),
                three: (0, 0),
            },
            Slot {
                one: None,
                two: (0, 0),
                three: (0, 0),
            },
            Slot {
                one: None,
                two: (0, 0),
                three: (0, 0),
            },
            Slot {
                one: None,
                two: (0, 0),
                three: (0, 0),
            },
            Slot {
                one: None,
                two: (0, 0),
                three: (0, 0),
            },
            Slot {
                one: None,
                two: (0, 0),
                three: (0, 0),
            },
            Slot {
                one: None,
                two: (0, 0),
                three: (0, 0),
            },
            Slot {
                one: None,
                two: (0, 0),
                three: (0, 0),
            },
            Slot {
                one: None,
                two: (0, 0),
                three: (0, 0),
            },
            Slot {
                one: None,
                two: (0, 0),
                three: (0, 0),
            },
            Slot {
                one: None,
                two: (0, 0),
                three: (0, 0),
            },
            Slot {
                one: None,
                two: (0, 0),
                three: (0, 0),
            },
            Slot {
                one: None,
                two: (0, 0),
                three: (0, 0),
            },
            Slot {
                one: None,
                two: (0, 0),
                three: (0, 0),
            },
            Slot {
                one: None,
                two: (0, 0),
                three: (0, 0),
            },
            Slot {
                one: None,
                two: (0, 0),
                three: (0, 0),
            },
            Slot {
                one: None,
                two: (0, 0),
                three: (0, 0),
            },
            Slot {
                one: None,
                two: (0, 0),
                three: (0, 0),
            },
            Slot {
                one: None,
                two: (0, 0),
                three: (0, 0),
            },
            Slot {
                one: None,
                two: (0, 0),
                three: (0, 0),
            },
            Slot {
                one: None,
                two: (0, 0),
                three: (0, 0),
            },
            Slot {
                one: None,
                two: (0, 0),
                three: (0, 0),
            },
            Slot {
                one: None,
                two: (0, 0),
                three: (0, 0),
            },
            Slot {
                one: None,
                two: (0, 0),
                three: (0, 0),
            },
            Slot {
                one: None,
                two: (0, 0),
                three: (0, 0),
            },
            Slot {
                one: None,
                two: (0, 0),
                three: (0, 0),
            },
            Slot {
                one: None,
                two: (0, 0),
                three: (0, 0),
            },
            Slot {
                one: None,
                two: (0, 0),
                three: (0, 0),
            },
            Slot {
                one: None,
                two: (0, 0),
                three: (0, 0),
            },
            Slot {
                one: None,
                two: (0, 0),
                three: (0, 0),
            },
            Slot {
                one: None,
                two: (0, 0),
                three: (0, 0),
            },
            Slot {
                one: None,
                two: (0, 0),
                three: (0, 0),
            },
            Slot {
                one: None,
                two: (0, 0),
                three: (0, 0),
            },
            Slot {
                one: None,
                two: (0, 0),
                three: (0, 0),
            },
            Slot {
                one: None,
                two: (0, 0),
                three: (0, 0),
            },
            Slot {
                one: None,
                two: (0, 0),
                three: (0, 0),
            },
            Slot {
                one: None,
                two: (5, 10),
                three: (0, 0),
            }, // ゔ
            Slot {
                one: None,
                two: (0, 0),
                three: (0, 0),
            },
            Slot {
                one: None,
                two: (0, 0),
                three: (0, 0),
            },
            Slot {
                one: None,
                two: (0, 0),
                three: (0, 0),
            },
            Slot {
                one: None,
                two: (0, 0),
                three: (0, 0),
            },
            Slot {
                one: None,
                two: (0, 0),
                three: (0, 0),
            },
            Slot {
                one: None,
                two: (0, 0),
                three: (0, 0),
            },
            Slot {
                one: None,
                two: (0, 0),
                three: (0, 0),
            },
            Slot {
                one: None,
                two: (0, 0),
                three: (0, 0),
            },
            Slot {
                one: None,
                two: (0, 0),
                three: (0, 0),
            },
            Slot {
                one: None,
                two: (0, 0),
                three: (0, 0),
            },
            Slot {
                one: None,
                two: (0, 0),
                three: (0, 0),
            },
            Slot {
                one: None,
                two: (0, 0),
                three: (0, 0),
            },
            Slot {
                one: None,
                two: (0, 0),
                three: (0, 0),
            },
            Slot {
                one: None,
                two: (0, 0),
                three: (0, 0),
            },
            Slot {
                one: None,
                two: (0, 0),
                three: (0, 0),
            },
            Slot {
                one: None,
                two: (0, 0),
                three: (0, 0),
            },
            Slot {
                one: None,
                two: (0, 0),
                three: (0, 0),
            },
            Slot {
                one: None,
                two: (10, 15),
                three: (0, 0),
            }, // ウ
            Slot {
                one: None,
                two: (0, 0),
                three: (0, 0),
            },
            Slot {
                one: None,
                two: (0, 0),
                three: (0, 0),
            },
            Slot {
                one: None,
                two: (0, 0),
                three: (0, 0),
            },
            Slot {
                one: None,
                two: (0, 0),
                three: (0, 0),
            },
            Slot {
                one: None,
                two: (0, 0),
                three: (0, 0),
            },
            Slot {
                one: None,
                two: (0, 0),
                three: (0, 0),
            },
            Slot {
                one: None,
                two: (0, 0),
                three: (0, 0),
            },
            Slot {
                one: None,
                two: (0, 0),
                three: (0, 0),
            },
            Slot {
                one: None,
                two: (0, 0),
                three: (0, 0),
            },
            Slot {
                one: None,
                two: (0, 0),
                three: (0, 0),
            },
            Slot {
                one: None,
                two: (0, 0),
                three: (0, 0),
            },
            Slot {
                one: None,
                two: (0, 0),
                three: (0, 0),
            },
            Slot {
                one: None,
                two: (0, 0),
                three: (0, 0),
            },
            Slot {
                one: None,
                two: (0, 0),
                three: (0, 0),
            },
            Slot {
                one: None,
                two: (0, 0),
                three: (0, 0),
            },
            Slot {
                one: None,
                two: (0, 0),
                three: (0, 0),
            },
            Slot {
                one: None,
                two: (0, 0),
                three: (0, 0),
            },
            Slot {
                one: None,
                two: (0, 0),
                three: (0, 0),
            },
            Slot {
                one: None,
                two: (0, 0),
                three: (0, 0),
            },
            Slot {
                one: None,
                two: (0, 0),
                three: (0, 0),
            },
            Slot {
                one: None,
                two: (0, 0),
                three: (0, 0),
            },
            Slot {
                one: None,
                two: (0, 0),
                three: (0, 0),
            },
            Slot {
                one: None,
                two: (0, 0),
                three: (0, 0),
            },
            Slot {
                one: None,
                two: (0, 0),
                three: (0, 0),
            },
            Slot {
                one: None,
                two: (0, 0),
                three: (0, 0),
            },
            Slot {
                one: None,
                two: (0, 0),
                three: (0, 0),
            },
            Slot {
                one: None,
                two: (0, 0),
                three: (0, 0),
            },
            Slot {
                one: None,
                two: (0, 0),
                three: (0, 0),
            },
            Slot {
                one: None,
                two: (0, 0),
                three: (0, 0),
            },
            Slot {
                one: None,
                two: (0, 0),
                three: (0, 0),
            },
            Slot {
                one: None,
                two: (0, 0),
                three: (0, 0),
            },
            Slot {
                one: None,
                two: (0, 0),
                three: (0, 0),
            },
            Slot {
                one: None,
                two: (0, 0),
                three: (0, 0),
            },
            Slot {
                one: None,
                two: (0, 0),
                three: (0, 0),
            },
            Slot {
                one: None,
                two: (0, 0),
                three: (0, 0),
            },
            Slot {
                one: None,
                two: (0, 0),
                three: (0, 0),
            },
            Slot {
                one: None,
                two: (0, 0),
                three: (0, 0),
            },
            Slot {
                one: None,
                two: (0, 0),
                three: (0, 0),
            },
            Slot {
                one: None,
                two: (0, 0),
                three: (0, 0),
            },
            Slot {
                one: None,
                two: (0, 0),
                three: (0, 0),
            },
            Slot {
                one: None,
                two: (0, 0),
                three: (0, 0),
            },
            Slot {
                one: None,
                two: (0, 0),
                three: (0, 0),
            },
            Slot {
                one: None,
                two: (0, 0),
                three: (0, 0),
            },
            Slot {
                one: None,
                two: (0, 0),
                three: (0, 0),
            },
            Slot {
                one: None,
                two: (0, 0),
                three: (0, 0),
            },
            Slot {
                one: None,
                two: (0, 0),
                three: (0, 0),
            },
            Slot {
                one: None,
                two: (0, 0),
                three: (0, 0),
            },
            Slot {
                one: None,
                two: (0, 0),
                three: (0, 0),
            },
            Slot {
                one: None,
                two: (0, 0),
                three: (0, 0),
            },
            Slot {
                one: None,
                two: (0, 0),
                three: (0, 0),
            },
            Slot {
                one: None,
                two: (0, 0),
                three: (0, 0),
            },
            Slot {
                one: None,
                two: (0, 0),
                three: (0, 0),
            },
            Slot {
                one: None,
                two: (0, 0),
                three: (0, 0),
            },
            Slot {
                one: None,
                two: (0, 0),
                three: (0, 0),
            },
            Slot {
                one: None,
                two: (0, 0),
                three: (0, 0),
            },
            Slot {
                one: None,
                two: (0, 0),
                three: (0, 0),
            },
            Slot {
                one: None,
                two: (0, 0),
                three: (0, 0),
            },
            Slot {
                one: None,
                two: (0, 0),
                three: (0, 0),
            },
            Slot {
                one: None,
                two: (0, 0),
                three: (0, 0),
            },
            Slot {
                one: None,
                two: (0, 0),
                three: (0, 0),
            },
            Slot {
                one: None,
                two: (0, 0),
                three: (0, 0),
            },
            Slot {
                one: None,
                two: (0, 0),
                three: (0, 0),
            },
            Slot {
                one: None,
                two: (0, 0),
                three: (0, 0),
            },
            Slot {
                one: None,
                two: (0, 0),
                three: (0, 0),
            },
            Slot {
                one: None,
                two: (0, 0),
                three: (0, 0),
            },
            Slot {
                one: None,
                two: (0, 0),
                three: (0, 0),
            },
            Slot {
                one: None,
                two: (0, 0),
                three: (0, 0),
            },
            Slot {
                one: None,
                two: (0, 0),
                three: (0, 0),
            },
            Slot {
                one: None,
                two: (0, 0),
                three: (0, 0),
            },
            Slot {
                one: None,
                two: (0, 0),
                three: (0, 0),
            },
            Slot {
                one: None,
                two: (0, 0),
                three: (0, 0),
            },
            Slot {
                one: None,
                two: (0, 0),
                three: (0, 0),
            },
            Slot {
                one: None,
                two: (0, 0),
                three: (0, 0),
            },
            Slot {
                one: None,
                two: (0, 0),
                three: (0, 0),
            },
            Slot {
                one: None,
                two: (0, 0),
                three: (0, 0),
            },
            Slot {
                one: None,
                two: (0, 0),
                three: (0, 0),
            },
            Slot {
                one: None,
                two: (0, 0),
                three: (0, 0),
            },
            Slot {
                one: None,
                two: (15, 20),
                three: (0, 0),
            }, // ヴ
        ],
    }],
    two: &[
        ('ぁ', "wa"),
        ('ぃ', "wi"),
        ('ぇ', "we"),
        ('ぉ', "wo"),
        ('ー', "ū"),
        ('ぁ', "va"),
        ('ぃ', "vi"),
        ('ぇ', "ve"),
        ('ぉ', "vo"),
        ('ゅ', "vyu"),
        ('ァ', "wa"),
        ('ィ', "wi"),
        ('ェ', "we"),
        ('ォ', "wo"),
        ('ー', "ū"),
        ('ァ', "va"),
        ('ィ', "vi"),
        ('ェ', "ve"),
        ('ォ', "vo"),
        ('ュ', "vyu"),
    ],
    three: &[],
};

/// Phase 3 (`replace2`): the main kana table, 191 katakana entries mirrored by 191 hiragana ones.
///
/// 382 entries: 149 one-`char`, 189 two-`char`, 44 three-`char`.
pub(crate) static TABLE2: Table = Table {
    windows: &[Window {
        base: 0x3042,
        slots: &[
            Slot {
                one: Some("a"),
                two: (0, 0),
                three: (0, 0),
            }, // あ
            Slot {
                one: None,
                two: (0, 0),
                three: (0, 0),
            },
            Slot {
                one: Some("i"),
                two: (0, 1),
                three: (0, 0),
            }, // い
            Slot {
                one: None,
                two: (0, 0),
                three: (0, 0),
            },
            Slot {
                one: Some("u"),
                two: (1, 2),
                three: (0, 0),
            }, // う
            Slot {
                one: None,
                two: (0, 0),
                three: (0, 0),
            },
            Slot {
                one: Some("e"),
                two: (0, 0),
                three: (0, 0),
            }, // え
            Slot {
                one: None,
                two: (0, 0),
                three: (0, 0),
            },
            Slot {
                one: Some("o"),
                two: (2, 3),
                three: (0, 0),
            }, // お
            Slot {
                one: Some("ka"),
                two: (0, 0),
                three: (0, 0),
            }, // か
            Slot {
                one: Some("ga"),
                two: (0, 0),
                three: (0, 0),
            }, // が
            Slot {
                one: Some("ki"),
                two: (3, 6),
                three: (0, 2),
            }, // き
            Slot {
                one: Some("gi"),
                two: (6, 9),
                three: (2, 4),
            }, // ぎ
            Slot {
                one: Some("ku"),
                two: (9, 14),
                three: (0, 0),
            }, // く
            Slot {
                one: Some("gu"),
                two: (14, 19),
                three: (0, 0),
            }, // ぐ
            Slot {
                one: Some("ke"),
                two: (0, 0),
                three: (0, 0),
            }, // け
            Slot {
                one: Some("ge"),
                two: (0, 0),
                three: (0, 0),
            }, // げ
            Slot {
                one: Some("ko"),
                two: (19, 20),
                three: (0, 0),
            }, // こ
            Slot {
                one: Some("go"),
                two: (20, 21),
                three: (0, 0),
            }, // ご
            Slot {
                one: Some("sa"),
                two: (0, 0),
                three: (0, 0),
            }, // さ
            Slot {
                one: Some("za"),
                two: (0, 0),
                three: (0, 0),
            }, // ざ
            Slot {
                one: Some("shi"),
                two: (21, 25),
                three: (4, 6),
            }, // し
            Slot {
                one: Some("ji"),
                two: (25, 29),
                three: (6, 8),
            }, // じ
            Slot {
                one: Some("su"),
                two: (29, 31),
                three: (0, 0),
            }, // す
            Slot {
                one: Some("zu"),
                two: (31, 33),
                three: (0, 0),
            }, // ず
            Slot {
                one: Some("se"),
                two: (0, 0),
                three: (0, 0),
            }, // せ
            Slot {
                one: Some("ze"),
                two: (0, 0),
                three: (0, 0),
            }, // ぜ
            Slot {
                one: Some("so"),
                two: (33, 34),
                three: (0, 0),
            }, // そ
            Slot {
                one: Some("zo"),
                two: (34, 35),
                three: (0, 0),
            }, // ぞ
            Slot {
                one: Some("ta"),
                two: (0, 0),
                three: (0, 0),
            }, // た
            Slot {
                one: Some("da"),
                two: (0, 0),
                three: (0, 0),
            }, // だ
            Slot {
                one: Some("chi"),
                two: (35, 39),
                three: (8, 10),
            }, // ち
            Slot {
                one: Some("ji"),
                two: (0, 0),
                three: (0, 0),
            }, // ぢ
            Slot {
                one: None,
                two: (0, 0),
                three: (0, 0),
            },
            Slot {
                one: Some("tsu"),
                two: (39, 44),
                three: (0, 0),
            }, // つ
            Slot {
                one: Some("zu"),
                two: (44, 45),
                three: (0, 0),
            }, // づ
            Slot {
                one: Some("te"),
                two: (45, 47),
                three: (0, 0),
            }, // て
            Slot {
                one: Some("de"),
                two: (47, 49),
                three: (0, 0),
            }, // で
            Slot {
                one: Some("to"),
                two: (49, 52),
                three: (0, 0),
            }, // と
            Slot {
                one: Some("do"),
                two: (52, 55),
                three: (0, 0),
            }, // ど
            Slot {
                one: Some("na"),
                two: (0, 0),
                three: (0, 0),
            }, // な
            Slot {
                one: Some("ni"),
                two: (55, 58),
                three: (10, 12),
            }, // に
            Slot {
                one: Some("nu"),
                two: (58, 59),
                three: (0, 0),
            }, // ぬ
            Slot {
                one: Some("ne"),
                two: (0, 0),
                three: (0, 0),
            }, // ね
            Slot {
                one: Some("no"),
                two: (59, 60),
                three: (0, 0),
            }, // の
            Slot {
                one: Some("ha"),
                two: (0, 0),
                three: (0, 0),
            }, // は
            Slot {
                one: Some("ba"),
                two: (0, 0),
                three: (0, 0),
            }, // ば
            Slot {
                one: Some("pa"),
                two: (0, 0),
                three: (0, 0),
            }, // ぱ
            Slot {
                one: Some("hi"),
                two: (60, 63),
                three: (12, 14),
            }, // ひ
            Slot {
                one: Some("bi"),
                two: (63, 66),
                three: (14, 16),
            }, // び
            Slot {
                one: Some("pi"),
                two: (66, 69),
                three: (16, 18),
            }, // ぴ
            Slot {
                one: Some("fu"),
                two: (69, 75),
                three: (0, 0),
            }, // ふ
            Slot {
                one: Some("bu"),
                two: (75, 77),
                three: (0, 0),
            }, // ぶ
            Slot {
                one: Some("pu"),
                two: (77, 78),
                three: (0, 0),
            }, // ぷ
            Slot {
                one: Some("he"),
                two: (0, 0),
                three: (0, 0),
            }, // へ
            Slot {
                one: Some("be"),
                two: (0, 0),
                three: (0, 0),
            }, // べ
            Slot {
                one: Some("pe"),
                two: (0, 0),
                three: (0, 0),
            }, // ぺ
            Slot {
                one: Some("ho"),
                two: (78, 80),
                three: (0, 0),
            }, // ほ
            Slot {
                one: Some("bo"),
                two: (80, 81),
                three: (0, 0),
            }, // ぼ
            Slot {
                one: Some("po"),
                two: (81, 82),
                three: (0, 0),
            }, // ぽ
            Slot {
                one: Some("ma"),
                two: (0, 0),
                three: (0, 0),
            }, // ま
            Slot {
                one: Some("mi"),
                two: (82, 85),
                three: (18, 20),
            }, // み
            Slot {
                one: Some("mu"),
                two: (85, 86),
                three: (0, 0),
            }, // む
            Slot {
                one: Some("me"),
                two: (0, 0),
                three: (0, 0),
            }, // め
            Slot {
                one: Some("mo"),
                two: (86, 87),
                three: (0, 0),
            }, // も
            Slot {
                one: None,
                two: (0, 0),
                three: (0, 0),
            },
            Slot {
                one: Some("ya"),
                two: (0, 0),
                three: (0, 0),
            }, // や
            Slot {
                one: None,
                two: (0, 0),
                three: (0, 0),
            },
            Slot {
                one: Some("yu"),
                two: (87, 88),
                three: (0, 0),
            }, // ゆ
            Slot {
                one: None,
                two: (0, 0),
                three: (0, 0),
            },
            Slot {
                one: Some("yo"),
                two: (88, 89),
                three: (0, 0),
            }, // よ
            Slot {
                one: Some("ra"),
                two: (0, 0),
                three: (0, 0),
            }, // ら
            Slot {
                one: Some("ri"),
                two: (89, 93),
                three: (20, 22),
            }, // り
            Slot {
                one: Some("ru"),
                two: (93, 94),
                three: (0, 0),
            }, // る
            Slot {
                one: Some("re"),
                two: (0, 0),
                three: (0, 0),
            }, // れ
            Slot {
                one: Some("ro"),
                two: (94, 95),
                three: (0, 0),
            }, // ろ
            Slot {
                one: None,
                two: (0, 0),
                three: (0, 0),
            },
            Slot {
                one: Some("wa"),
                two: (0, 0),
                three: (0, 0),
            }, // わ
            Slot {
                one: Some("i"),
                two: (0, 0),
                three: (0, 0),
            }, // ゐ
            Slot {
                one: Some("e"),
                two: (0, 0),
                three: (0, 0),
            }, // ゑ
            Slot {
                one: Some("o"),
                two: (0, 0),
                three: (0, 0),
            }, // を
            Slot {
                one: Some("n"),
                two: (0, 0),
                three: (0, 0),
            }, // ん
            Slot {
                one: Some("v"),
                two: (0, 0),
                three: (0, 0),
            }, // ゔ
            Slot {
                one: None,
                two: (0, 0),
                three: (0, 0),
            },
            Slot {
                one: None,
                two: (0, 0),
                three: (0, 0),
            },
            Slot {
                one: None,
                two: (0, 0),
                three: (0, 0),
            },
            Slot {
                one: None,
                two: (0, 0),
                three: (0, 0),
            },
            Slot {
                one: None,
                two: (0, 0),
                three: (0, 0),
            },
            Slot {
                one: None,
                two: (0, 0),
                three: (0, 0),
            },
            Slot {
                one: None,
                two: (0, 0),
                three: (0, 0),
            },
            Slot {
                one: None,
                two: (0, 0),
                three: (0, 0),
            },
            Slot {
                one: None,
                two: (0, 0),
                three: (0, 0),
            },
            Slot {
                one: None,
                two: (0, 0),
                three: (0, 0),
            },
            Slot {
                one: None,
                two: (0, 0),
                three: (0, 0),
            },
            Slot {
                one: None,
                two: (0, 0),
                three: (0, 0),
            },
            Slot {
                one: None,
                two: (0, 0),
                three: (0, 0),
            },
            Slot {
                one: Some("a"),
                two: (0, 0),
                three: (0, 0),
            }, // ア
            Slot {
                one: None,
                two: (0, 0),
                three: (0, 0),
            },
            Slot {
                one: Some("i"),
                two: (95, 96),
                three: (0, 0),
            }, // イ
            Slot {
                one: None,
                two: (0, 0),
                three: (0, 0),
            },
            Slot {
                one: Some("u"),
                two: (96, 97),
                three: (0, 0),
            }, // ウ
            Slot {
                one: None,
                two: (0, 0),
                three: (0, 0),
            },
            Slot {
                one: Some("e"),
                two: (0, 0),
                three: (0, 0),
            }, // エ
            Slot {
                one: None,
                two: (0, 0),
                three: (0, 0),
            },
            Slot {
                one: Some("o"),
                two: (97, 98),
                three: (0, 0),
            }, // オ
            Slot {
                one: Some("ka"),
                two: (0, 0),
                three: (0, 0),
            }, // カ
            Slot {
                one: Some("ga"),
                two: (0, 0),
                three: (0, 0),
            }, // ガ
            Slot {
                one: Some("ki"),
                two: (98, 101),
                three: (22, 24),
            }, // キ
            Slot {
                one: Some("gi"),
                two: (101, 104),
                three: (24, 26),
            }, // ギ
            Slot {
                one: Some("ku"),
                two: (104, 108),
                three: (0, 0),
            }, // ク
            Slot {
                one: Some("gu"),
                two: (108, 113),
                three: (0, 0),
            }, // グ
            Slot {
                one: Some("ke"),
                two: (0, 0),
                three: (0, 0),
            }, // ケ
            Slot {
                one: Some("ge"),
                two: (0, 0),
                three: (0, 0),
            }, // ゲ
            Slot {
                one: Some("ko"),
                two: (113, 114),
                three: (0, 0),
            }, // コ
            Slot {
                one: Some("go"),
                two: (114, 115),
                three: (0, 0),
            }, // ゴ
            Slot {
                one: Some("sa"),
                two: (0, 0),
                three: (0, 0),
            }, // サ
            Slot {
                one: Some("za"),
                two: (0, 0),
                three: (0, 0),
            }, // ザ
            Slot {
                one: Some("shi"),
                two: (115, 119),
                three: (26, 28),
            }, // シ
            Slot {
                one: Some("ji"),
                two: (119, 123),
                three: (28, 30),
            }, // ジ
            Slot {
                one: Some("su"),
                two: (123, 125),
                three: (0, 0),
            }, // ス
            Slot {
                one: Some("zu"),
                two: (125, 127),
                three: (0, 0),
            }, // ズ
            Slot {
                one: Some("se"),
                two: (0, 0),
                three: (0, 0),
            }, // セ
            Slot {
                one: Some("ze"),
                two: (0, 0),
                three: (0, 0),
            }, // ゼ
            Slot {
                one: Some("so"),
                two: (127, 128),
                three: (0, 0),
            }, // ソ
            Slot {
                one: Some("zo"),
                two: (128, 129),
                three: (0, 0),
            }, // ゾ
            Slot {
                one: Some("ta"),
                two: (0, 0),
                three: (0, 0),
            }, // タ
            Slot {
                one: Some("da"),
                two: (0, 0),
                three: (0, 0),
            }, // ダ
            Slot {
                one: Some("chi"),
                two: (129, 133),
                three: (30, 32),
            }, // チ
            Slot {
                one: Some("ji"),
                two: (0, 0),
                three: (0, 0),
            }, // ヂ
            Slot {
                one: None,
                two: (0, 0),
                three: (0, 0),
            },
            Slot {
                one: Some("tsu"),
                two: (133, 138),
                three: (0, 0),
            }, // ツ
            Slot {
                one: Some("zu"),
                two: (138, 139),
                three: (0, 0),
            }, // ヅ
            Slot {
                one: Some("te"),
                two: (139, 141),
                three: (0, 0),
            }, // テ
            Slot {
                one: Some("de"),
                two: (141, 143),
                three: (0, 0),
            }, // デ
            Slot {
                one: Some("to"),
                two: (143, 146),
                three: (0, 0),
            }, // ト
            Slot {
                one: Some("do"),
                two: (146, 149),
                three: (0, 0),
            }, // ド
            Slot {
                one: Some("na"),
                two: (0, 0),
                three: (0, 0),
            }, // ナ
            Slot {
                one: Some("ni"),
                two: (149, 152),
                three: (32, 34),
            }, // ニ
            Slot {
                one: Some("nu"),
                two: (152, 153),
                three: (0, 0),
            }, // ヌ
            Slot {
                one: Some("ne"),
                two: (0, 0),
                three: (0, 0),
            }, // ネ
            Slot {
                one: Some("no"),
                two: (153, 154),
                three: (0, 0),
            }, // ノ
            Slot {
                one: Some("ha"),
                two: (0, 0),
                three: (0, 0),
            }, // ハ
            Slot {
                one: Some("ba"),
                two: (0, 0),
                three: (0, 0),
            }, // バ
            Slot {
                one: Some("pa"),
                two: (0, 0),
                three: (0, 0),
            }, // パ
            Slot {
                one: Some("hi"),
                two: (154, 157),
                three: (34, 36),
            }, // ヒ
            Slot {
                one: Some("bi"),
                two: (157, 160),
                three: (36, 38),
            }, // ビ
            Slot {
                one: Some("pi"),
                two: (160, 163),
                three: (38, 40),
            }, // ピ
            Slot {
                one: Some("fu"),
                two: (163, 169),
                three: (0, 0),
            }, // フ
            Slot {
                one: Some("bu"),
                two: (169, 171),
                three: (0, 0),
            }, // ブ
            Slot {
                one: Some("pu"),
                two: (171, 172),
                three: (0, 0),
            }, // プ
            Slot {
                one: Some("he"),
                two: (0, 0),
                three: (0, 0),
            }, // ヘ
            Slot {
                one: Some("be"),
                two: (0, 0),
                three: (0, 0),
            }, // ベ
            Slot {
                one: Some("pe"),
                two: (0, 0),
                three: (0, 0),
            }, // ペ
            Slot {
                one: Some("ho"),
                two: (172, 174),
                three: (0, 0),
            }, // ホ
            Slot {
                one: Some("bo"),
                two: (174, 175),
                three: (0, 0),
            }, // ボ
            Slot {
                one: Some("po"),
                two: (175, 176),
                three: (0, 0),
            }, // ポ
            Slot {
                one: Some("ma"),
                two: (0, 0),
                three: (0, 0),
            }, // マ
            Slot {
                one: Some("mi"),
                two: (176, 179),
                three: (40, 42),
            }, // ミ
            Slot {
                one: Some("mu"),
                two: (179, 180),
                three: (0, 0),
            }, // ム
            Slot {
                one: Some("me"),
                two: (0, 0),
                three: (0, 0),
            }, // メ
            Slot {
                one: Some("mo"),
                two: (180, 181),
                three: (0, 0),
            }, // モ
            Slot {
                one: None,
                two: (0, 0),
                three: (0, 0),
            },
            Slot {
                one: Some("ya"),
                two: (0, 0),
                three: (0, 0),
            }, // ヤ
            Slot {
                one: None,
                two: (0, 0),
                three: (0, 0),
            },
            Slot {
                one: Some("yu"),
                two: (181, 182),
                three: (0, 0),
            }, // ユ
            Slot {
                one: None,
                two: (0, 0),
                three: (0, 0),
            },
            Slot {
                one: Some("yo"),
                two: (182, 183),
                three: (0, 0),
            }, // ヨ
            Slot {
                one: Some("ra"),
                two: (0, 0),
                three: (0, 0),
            }, // ラ
            Slot {
                one: Some("ri"),
                two: (183, 187),
                three: (42, 44),
            }, // リ
            Slot {
                one: Some("ru"),
                two: (187, 188),
                three: (0, 0),
            }, // ル
            Slot {
                one: Some("re"),
                two: (0, 0),
                three: (0, 0),
            }, // レ
            Slot {
                one: Some("ro"),
                two: (188, 189),
                three: (0, 0),
            }, // ロ
            Slot {
                one: None,
                two: (0, 0),
                three: (0, 0),
            },
            Slot {
                one: Some("wa"),
                two: (0, 0),
                three: (0, 0),
            }, // ワ
            Slot {
                one: Some("i"),
                two: (0, 0),
                three: (0, 0),
            }, // ヰ
            Slot {
                one: Some("e"),
                two: (0, 0),
                three: (0, 0),
            }, // ヱ
            Slot {
                one: Some("o"),
                two: (0, 0),
                three: (0, 0),
            }, // ヲ
            Slot {
                one: Some("n"),
                two: (0, 0),
                three: (0, 0),
            }, // ン
            Slot {
                one: Some("v"),
                two: (0, 0),
                three: (0, 0),
            }, // ヴ
            Slot {
                one: None,
                two: (0, 0),
                three: (0, 0),
            },
            Slot {
                one: None,
                two: (0, 0),
                three: (0, 0),
            },
            Slot {
                one: None,
                two: (0, 0),
                three: (0, 0),
            },
            Slot {
                one: None,
                two: (0, 0),
                three: (0, 0),
            },
            Slot {
                one: None,
                two: (0, 0),
                three: (0, 0),
            },
            Slot {
                one: None,
                two: (0, 0),
                three: (0, 0),
            },
            Slot {
                one: Some(" "),
                two: (0, 0),
                three: (0, 0),
            }, // ・
        ],
    }],
    two: &[
        ('ぇ', "ye"),
        ('う', "ū"),
        ('う', "ō"),
        ('ゃ', "kya"),
        ('ゅ', "kyu"),
        ('ょ', "kyo"),
        ('ゃ', "gya"),
        ('ゅ', "gyu"),
        ('ょ', "gyo"),
        ('ぁ', "kwa"),
        ('ぃ', "kwi"),
        ('う', "kū"),
        ('ぇ', "kwe"),
        ('ぉ', "kwo"),
        ('ぁ', "gwa"),
        ('ぃ', "gwi"),
        ('う', "gū"),
        ('ぇ', "gwe"),
        ('ぉ', "gwo"),
        ('う', "kō"),
        ('う', "gō"),
        ('ぇ', "she"),
        ('ゃ', "sha"),
        ('ゅ', "shu"),
        ('ょ', "sho"),
        ('ぇ', "je"),
        ('ゃ', "ja"),
        ('ゅ', "ju"),
        ('ょ', "jo"),
        ('ぃ', "si"),
        ('う', "sū"),
        ('ぃ', "zi"),
        ('う', "zū"),
        ('う', "sō"),
        ('う', "zō"),
        ('ぇ', "che"),
        ('ゃ', "cha"),
        ('ゅ', "chu"),
        ('ょ', "cho"),
        ('ぁ', "tsa"),
        ('ぃ', "tsi"),
        ('う', "tsū"),
        ('ぇ', "tse"),
        ('ぉ', "tso"),
        ('う', "zū"),
        ('ぃ', "ti"),
        ('ゅ', "tyu"),
        ('ぃ', "di"),
        ('ゅ', "dyu"),
        ('ぃ', "twi"),
        ('ぅ', "tu"),
        ('う', "tō"),
        ('ぃ', "dwi"),
        ('ぅ', "du"),
        ('う', "dō"),
        ('ゃ', "nya"),
        ('ゅ', "nyu"),
        ('ょ', "nyo"),
        ('う', "nū"),
        ('う', "nō"),
        ('ゃ', "hya"),
        ('ゅ', "hyu"),
        ('ょ', "hyo"),
        ('ゃ', "bya"),
        ('ゅ', "byu"),
        ('ょ', "byo"),
        ('ゃ', "pya"),
        ('ゅ', "pyu"),
        ('ょ', "pyo"),
        ('ぁ', "fa"),
        ('ぃ', "fi"),
        ('う', "fū"),
        ('ぇ', "fe"),
        ('ぉ', "fo"),
        ('ゅ', "fyu"),
        ('う', "bū"),
        ('ゅ', "byu"),
        ('う', "pū"),
        ('う', "hō"),
        ('ぇ', "hwe"),
        ('う', "bō"),
        ('う', "pō"),
        ('ゃ', "mya"),
        ('ゅ', "myu"),
        ('ょ', "myo"),
        ('う', "mū"),
        ('う', "mō"),
        ('う', "yū"),
        ('う', "yō"),
        ('ぇ', "rye"),
        ('ゃ', "rya"),
        ('ゅ', "ryu"),
        ('ょ', "ryo"),
        ('う', "rū"),
        ('う', "rō"),
        ('ェ', "ye"),
        ('ウ', "ū"),
        ('ウ', "ō"),
        ('ャ', "kya"),
        ('ュ', "kyu"),
        ('ョ', "kyo"),
        ('ャ', "gya"),
        ('ュ', "gyu"),
        ('ョ', "gyo"),
        ('ァ', "kwa"),
        ('ィ', "kwi"),
        ('ェ', "kwe"),
        ('ォ', "kwo"),
        ('ァ', "gwa"),
        ('ィ', "gwi"),
        ('ウ', "gū"),
        ('ェ', "gwe"),
        ('ォ', "gwo"),
        ('ウ', "kō"),
        ('ウ', "gō"),
        ('ェ', "she"),
        ('ャ', "sha"),
        ('ュ', "shu"),
        ('ョ', "sho"),
        ('ェ', "je"),
        ('ャ', "ja"),
        ('ュ', "ju"),
        ('ョ', "jo"),
        ('ィ', "si"),
        ('ウ', "sū"),
        ('ィ', "zi"),
        ('ウ', "zū"),
        ('ウ', "sō"),
        ('ウ', "zō"),
        ('ェ', "che"),
        ('ャ', "cha"),
        ('ュ', "chu"),
        ('ョ', "cho"),
        ('ァ', "tsa"),
        ('ィ', "tsi"),
        ('ウ', "tsū"),
        ('ェ', "tse"),
        ('ォ', "tso"),
        ('ウ', "zū"),
        ('ィ', "ti"),
        ('ュ', "tyu"),
        ('ィ', "di"),
        ('ュ', "dyu"),
        ('ィ', "twi"),
        ('ゥ', "tu"),
        ('ウ', "tō"),
        ('ィ', "dwi"),
        ('ゥ', "du"),
        ('ウ', "dō"),
        ('ャ', "nya"),
        ('ュ', "nyu"),
        ('ョ', "nyo"),
        ('ウ', "nū"),
        ('ウ', "nō"),
        ('ャ', "hya"),
        ('ュ', "hyu"),
        ('ョ', "hyo"),
        ('ャ', "bya"),
        ('ュ', "byu"),
        ('ョ', "byo"),
        ('ャ', "pya"),
        ('ュ', "pyu"),
        ('ョ', "pyo"),
        ('ァ', "fa"),
        ('ィ', "fi"),
        ('ウ', "fū"),
        ('ェ', "fe"),
        ('ォ', "fo"),
        ('ュ', "fyu"),
        ('ウ', "bū"),
        ('ュ', "byu"),
        ('ウ', "pū"),
        ('ウ', "hō"),
        ('ェ', "hwe"),
        ('ウ', "bō"),
        ('ウ', "pō"),
        ('ャ', "mya"),
        ('ュ', "myu"),
        ('ョ', "myo"),
        ('ウ', "mū"),
        ('ウ', "mō"),
        ('ウ', "yū"),
        ('ウ', "yō"),
        ('ェ', "rye"),
        ('ャ', "rya"),
        ('ュ', "ryu"),
        ('ョ', "ryo"),
        ('ウ', "rū"),
        ('ウ', "rō"),
    ],
    three: &[
        (['ゅ', 'う'], "kyū"),
        (['ょ', 'う'], "kyō"),
        (['ゅ', 'う'], "gyū"),
        (['ょ', 'う'], "gyō"),
        (['ゅ', 'う'], "shū"),
        (['ょ', 'う'], "shō"),
        (['ゅ', 'う'], "jū"),
        (['ょ', 'う'], "jō"),
        (['ゅ', 'う'], "chū"),
        (['ょ', 'う'], "chō"),
        (['ゅ', 'う'], "nyū"),
        (['ょ', 'う'], "nyō"),
        (['ゅ', 'う'], "hyū"),
        (['ょ', 'う'], "hyō"),
        (['ゅ', 'う'], "byū"),
        (['ょ', 'う'], "byō"),
        (['ゅ', 'う'], "pyū"),
        (['ょ', 'う'], "pyō"),
        (['ゅ', 'う'], "myū"),
        (['ょ', 'う'], "myō"),
        (['ゅ', 'う'], "ryū"),
        (['ょ', 'う'], "ryō"),
        (['ュ', 'ウ'], "kyū"),
        (['ョ', 'ウ'], "kyō"),
        (['ュ', 'ウ'], "gyū"),
        (['ョ', 'ウ'], "gyō"),
        (['ュ', 'ウ'], "shū"),
        (['ョ', 'ウ'], "shō"),
        (['ュ', 'ウ'], "jū"),
        (['ョ', 'ウ'], "jō"),
        (['ュ', 'ウ'], "chū"),
        (['ョ', 'ウ'], "chō"),
        (['ュ', 'ウ'], "nyū"),
        (['ョ', 'ウ'], "nyō"),
        (['ュ', 'ウ'], "hyū"),
        (['ョ', 'ウ'], "hyō"),
        (['ュ', 'ウ'], "byū"),
        (['ョ', 'ウ'], "byō"),
        (['ュ', 'ウ'], "pyū"),
        (['ョ', 'ウ'], "pyō"),
        (['ュ', 'ウ'], "myū"),
        (['ョ', 'ウ'], "myō"),
        (['ュ', 'ウ'], "ryū"),
        (['ョ', 'ウ'], "ryō"),
    ],
};

/// Phase 4 (`replace3`): long vowels and the small-vowel fallback, run over the MIXED latin+kana string.
///
/// 21 entries: 10 one-`char`, 9 two-`char`, 2 three-`char`.
pub(crate) static TABLE3: Table = Table {
    windows: &[
        Window {
            base: 0x61,
            slots: &[
                Slot {
                    one: None,
                    two: (0, 3),
                    three: (0, 0),
                }, // a
                Slot {
                    one: None,
                    two: (0, 0),
                    three: (0, 0),
                },
                Slot {
                    one: None,
                    two: (0, 0),
                    three: (0, 0),
                },
                Slot {
                    one: None,
                    two: (0, 0),
                    three: (0, 0),
                },
                Slot {
                    one: None,
                    two: (3, 4),
                    three: (0, 0),
                }, // e
                Slot {
                    one: None,
                    two: (0, 0),
                    three: (0, 0),
                },
                Slot {
                    one: None,
                    two: (0, 0),
                    three: (0, 0),
                },
                Slot {
                    one: None,
                    two: (0, 0),
                    three: (0, 0),
                },
                Slot {
                    one: None,
                    two: (4, 7),
                    three: (0, 2),
                }, // i
                Slot {
                    one: None,
                    two: (0, 0),
                    three: (0, 0),
                },
                Slot {
                    one: None,
                    two: (0, 0),
                    three: (0, 0),
                },
                Slot {
                    one: None,
                    two: (0, 0),
                    three: (0, 0),
                },
                Slot {
                    one: None,
                    two: (0, 0),
                    three: (0, 0),
                },
                Slot {
                    one: None,
                    two: (0, 0),
                    three: (0, 0),
                },
                Slot {
                    one: None,
                    two: (7, 8),
                    three: (0, 0),
                }, // o
                Slot {
                    one: None,
                    two: (0, 0),
                    three: (0, 0),
                },
                Slot {
                    one: None,
                    two: (0, 0),
                    three: (0, 0),
                },
                Slot {
                    one: None,
                    two: (0, 0),
                    three: (0, 0),
                },
                Slot {
                    one: None,
                    two: (0, 0),
                    three: (0, 0),
                },
                Slot {
                    one: None,
                    two: (0, 0),
                    three: (0, 0),
                },
                Slot {
                    one: None,
                    two: (8, 9),
                    three: (0, 0),
                }, // u
            ],
        },
        Window {
            base: 0x3041,
            slots: &[
                Slot {
                    one: Some("a"),
                    two: (0, 0),
                    three: (0, 0),
                }, // ぁ
                Slot {
                    one: None,
                    two: (0, 0),
                    three: (0, 0),
                },
                Slot {
                    one: Some("i"),
                    two: (0, 0),
                    three: (0, 0),
                }, // ぃ
                Slot {
                    one: None,
                    two: (0, 0),
                    three: (0, 0),
                },
                Slot {
                    one: Some("u"),
                    two: (0, 0),
                    three: (0, 0),
                }, // ぅ
                Slot {
                    one: None,
                    two: (0, 0),
                    three: (0, 0),
                },
                Slot {
                    one: Some("e"),
                    two: (0, 0),
                    three: (0, 0),
                }, // ぇ
                Slot {
                    one: None,
                    two: (0, 0),
                    three: (0, 0),
                },
                Slot {
                    one: Some("o"),
                    two: (0, 0),
                    three: (0, 0),
                }, // ぉ
                Slot {
                    one: None,
                    two: (0, 0),
                    three: (0, 0),
                },
                Slot {
                    one: None,
                    two: (0, 0),
                    three: (0, 0),
                },
                Slot {
                    one: None,
                    two: (0, 0),
                    three: (0, 0),
                },
                Slot {
                    one: None,
                    two: (0, 0),
                    three: (0, 0),
                },
                Slot {
                    one: None,
                    two: (0, 0),
                    three: (0, 0),
                },
                Slot {
                    one: None,
                    two: (0, 0),
                    three: (0, 0),
                },
                Slot {
                    one: None,
                    two: (0, 0),
                    three: (0, 0),
                },
                Slot {
                    one: None,
                    two: (0, 0),
                    three: (0, 0),
                },
                Slot {
                    one: None,
                    two: (0, 0),
                    three: (0, 0),
                },
                Slot {
                    one: None,
                    two: (0, 0),
                    three: (0, 0),
                },
                Slot {
                    one: None,
                    two: (0, 0),
                    three: (0, 0),
                },
                Slot {
                    one: None,
                    two: (0, 0),
                    three: (0, 0),
                },
                Slot {
                    one: None,
                    two: (0, 0),
                    three: (0, 0),
                },
                Slot {
                    one: None,
                    two: (0, 0),
                    three: (0, 0),
                },
                Slot {
                    one: None,
                    two: (0, 0),
                    three: (0, 0),
                },
                Slot {
                    one: None,
                    two: (0, 0),
                    three: (0, 0),
                },
                Slot {
                    one: None,
                    two: (0, 0),
                    three: (0, 0),
                },
                Slot {
                    one: None,
                    two: (0, 0),
                    three: (0, 0),
                },
                Slot {
                    one: None,
                    two: (0, 0),
                    three: (0, 0),
                },
                Slot {
                    one: None,
                    two: (0, 0),
                    three: (0, 0),
                },
                Slot {
                    one: None,
                    two: (0, 0),
                    three: (0, 0),
                },
                Slot {
                    one: None,
                    two: (0, 0),
                    three: (0, 0),
                },
                Slot {
                    one: None,
                    two: (0, 0),
                    three: (0, 0),
                },
                Slot {
                    one: None,
                    two: (0, 0),
                    three: (0, 0),
                },
                Slot {
                    one: None,
                    two: (0, 0),
                    three: (0, 0),
                },
                Slot {
                    one: None,
                    two: (0, 0),
                    three: (0, 0),
                },
                Slot {
                    one: None,
                    two: (0, 0),
                    three: (0, 0),
                },
                Slot {
                    one: None,
                    two: (0, 0),
                    three: (0, 0),
                },
                Slot {
                    one: None,
                    two: (0, 0),
                    three: (0, 0),
                },
                Slot {
                    one: None,
                    two: (0, 0),
                    three: (0, 0),
                },
                Slot {
                    one: None,
                    two: (0, 0),
                    three: (0, 0),
                },
                Slot {
                    one: None,
                    two: (0, 0),
                    three: (0, 0),
                },
                Slot {
                    one: None,
                    two: (0, 0),
                    three: (0, 0),
                },
                Slot {
                    one: None,
                    two: (0, 0),
                    three: (0, 0),
                },
                Slot {
                    one: None,
                    two: (0, 0),
                    three: (0, 0),
                },
                Slot {
                    one: None,
                    two: (0, 0),
                    three: (0, 0),
                },
                Slot {
                    one: None,
                    two: (0, 0),
                    three: (0, 0),
                },
                Slot {
                    one: None,
                    two: (0, 0),
                    three: (0, 0),
                },
                Slot {
                    one: None,
                    two: (0, 0),
                    three: (0, 0),
                },
                Slot {
                    one: None,
                    two: (0, 0),
                    three: (0, 0),
                },
                Slot {
                    one: None,
                    two: (0, 0),
                    three: (0, 0),
                },
                Slot {
                    one: None,
                    two: (0, 0),
                    three: (0, 0),
                },
                Slot {
                    one: None,
                    two: (0, 0),
                    three: (0, 0),
                },
                Slot {
                    one: None,
                    two: (0, 0),
                    three: (0, 0),
                },
                Slot {
                    one: None,
                    two: (0, 0),
                    three: (0, 0),
                },
                Slot {
                    one: None,
                    two: (0, 0),
                    three: (0, 0),
                },
                Slot {
                    one: None,
                    two: (0, 0),
                    three: (0, 0),
                },
                Slot {
                    one: None,
                    two: (0, 0),
                    three: (0, 0),
                },
                Slot {
                    one: None,
                    two: (0, 0),
                    three: (0, 0),
                },
                Slot {
                    one: None,
                    two: (0, 0),
                    three: (0, 0),
                },
                Slot {
                    one: None,
                    two: (0, 0),
                    three: (0, 0),
                },
                Slot {
                    one: None,
                    two: (0, 0),
                    three: (0, 0),
                },
                Slot {
                    one: None,
                    two: (0, 0),
                    three: (0, 0),
                },
                Slot {
                    one: None,
                    two: (0, 0),
                    three: (0, 0),
                },
                Slot {
                    one: None,
                    two: (0, 0),
                    three: (0, 0),
                },
                Slot {
                    one: None,
                    two: (0, 0),
                    three: (0, 0),
                },
                Slot {
                    one: None,
                    two: (0, 0),
                    three: (0, 0),
                },
                Slot {
                    one: None,
                    two: (0, 0),
                    three: (0, 0),
                },
                Slot {
                    one: None,
                    two: (0, 0),
                    three: (0, 0),
                },
                Slot {
                    one: None,
                    two: (0, 0),
                    three: (0, 0),
                },
                Slot {
                    one: None,
                    two: (0, 0),
                    three: (0, 0),
                },
                Slot {
                    one: None,
                    two: (0, 0),
                    three: (0, 0),
                },
                Slot {
                    one: None,
                    two: (0, 0),
                    three: (0, 0),
                },
                Slot {
                    one: None,
                    two: (0, 0),
                    three: (0, 0),
                },
                Slot {
                    one: None,
                    two: (0, 0),
                    three: (0, 0),
                },
                Slot {
                    one: None,
                    two: (0, 0),
                    three: (0, 0),
                },
                Slot {
                    one: None,
                    two: (0, 0),
                    three: (0, 0),
                },
                Slot {
                    one: None,
                    two: (0, 0),
                    three: (0, 0),
                },
                Slot {
                    one: None,
                    two: (0, 0),
                    three: (0, 0),
                },
                Slot {
                    one: None,
                    two: (0, 0),
                    three: (0, 0),
                },
                Slot {
                    one: None,
                    two: (0, 0),
                    three: (0, 0),
                },
                Slot {
                    one: None,
                    two: (0, 0),
                    three: (0, 0),
                },
                Slot {
                    one: None,
                    two: (0, 0),
                    three: (0, 0),
                },
                Slot {
                    one: None,
                    two: (0, 0),
                    three: (0, 0),
                },
                Slot {
                    one: None,
                    two: (0, 0),
                    three: (0, 0),
                },
                Slot {
                    one: None,
                    two: (0, 0),
                    three: (0, 0),
                },
                Slot {
                    one: None,
                    two: (0, 0),
                    three: (0, 0),
                },
                Slot {
                    one: None,
                    two: (0, 0),
                    three: (0, 0),
                },
                Slot {
                    one: None,
                    two: (0, 0),
                    three: (0, 0),
                },
                Slot {
                    one: None,
                    two: (0, 0),
                    three: (0, 0),
                },
                Slot {
                    one: None,
                    two: (0, 0),
                    three: (0, 0),
                },
                Slot {
                    one: None,
                    two: (0, 0),
                    three: (0, 0),
                },
                Slot {
                    one: None,
                    two: (0, 0),
                    three: (0, 0),
                },
                Slot {
                    one: None,
                    two: (0, 0),
                    three: (0, 0),
                },
                Slot {
                    one: None,
                    two: (0, 0),
                    three: (0, 0),
                },
                Slot {
                    one: None,
                    two: (0, 0),
                    three: (0, 0),
                },
                Slot {
                    one: None,
                    two: (0, 0),
                    three: (0, 0),
                },
                Slot {
                    one: Some("a"),
                    two: (0, 0),
                    three: (0, 0),
                }, // ァ
                Slot {
                    one: None,
                    two: (0, 0),
                    three: (0, 0),
                },
                Slot {
                    one: Some("i"),
                    two: (0, 0),
                    three: (0, 0),
                }, // ィ
                Slot {
                    one: None,
                    two: (0, 0),
                    three: (0, 0),
                },
                Slot {
                    one: Some("u"),
                    two: (0, 0),
                    three: (0, 0),
                }, // ゥ
                Slot {
                    one: None,
                    two: (0, 0),
                    three: (0, 0),
                },
                Slot {
                    one: Some("e"),
                    two: (0, 0),
                    three: (0, 0),
                }, // ェ
                Slot {
                    one: None,
                    two: (0, 0),
                    three: (0, 0),
                },
                Slot {
                    one: Some("o"),
                    two: (0, 0),
                    three: (0, 0),
                }, // ォ
            ],
        },
    ],
    two: &[
        ('ぁ', "ā"),
        ('ァ', "ā"),
        ('ー', "ā"),
        ('ー', "ē"),
        ('ぃ', "ī"),
        ('ィ', "ī"),
        ('ー', "ī"),
        ('ー', "ō"),
        ('ー', "ū"),
    ],
    three: &[(['ぃ', 'ー'], "ī"), (['ィ', 'ー'], "ī")],
};

/// The 30 `/SRC(?=[CLASS])/g` rules of phase 2, flattened.
///
/// Outer entries are sorted by source `char`, inner entries by the lookahead
/// `char`, so both lookups are a binary search. Flattening is only sound
/// because the classes belonging to one source character are pairwise disjoint;
/// This table was refused at derivation time if that ever failed, since an
/// overlap would silently discard the reference's source-order tie-break.
pub(crate) static LOOKAHEAD: &[(char, &[(char, &str)])] = &[
    (
        'っ',
        &[
            ('か', "k"),
            ('が', "g"),
            ('き', "k"),
            ('ぎ', "g"),
            ('く', "k"),
            ('ぐ', "g"),
            ('け', "k"),
            ('げ', "g"),
            ('こ', "k"),
            ('ご', "g"),
            ('さ', "s"),
            ('ざ', "z"),
            ('し', "s"),
            ('じ', "j"),
            ('す', "s"),
            ('ず', "z"),
            ('せ', "s"),
            ('ぜ', "z"),
            ('そ', "s"),
            ('ぞ', "z"),
            ('た', "t"),
            ('だ', "t"),
            ('ち', "t"),
            ('ぢ', "t"),
            ('つ', "t"),
            ('づ', "t"),
            ('て', "t"),
            ('で', "t"),
            ('と', "t"),
            ('ど', "t"),
            ('は', "h"),
            ('ば', "b"),
            ('ぱ', "p"),
            ('ひ', "h"),
            ('び', "b"),
            ('ぴ', "p"),
            ('ふ', "f"),
            ('ぶ', "b"),
            ('ぷ', "p"),
            ('へ', "h"),
            ('べ', "b"),
            ('ぺ', "p"),
            ('ほ', "h"),
            ('ぼ', "b"),
            ('ぽ', "p"),
            ('ら', "r"),
            ('り', "r"),
            ('る', "r"),
            ('れ', "r"),
            ('ろ', "r"),
            ('ん', "n"),
        ],
    ),
    (
        'ん',
        &[
            ('あ', "n'"),
            ('い', "n'"),
            ('う', "n'"),
            ('え', "n'"),
            ('お', "n'"),
            ('ば', "m"),
            ('ぱ', "m"),
            ('び', "m"),
            ('ぴ', "m"),
            ('ぶ', "m"),
            ('ぷ', "m"),
            ('べ', "m"),
            ('ぺ', "m"),
            ('ぼ', "m"),
            ('ぽ', "m"),
            ('ま', "m"),
            ('み', "m"),
            ('む', "m"),
            ('め', "m"),
            ('も', "m"),
            ('や', "n'"),
            ('ゆ', "n'"),
            ('よ', "n'"),
        ],
    ),
    (
        'ッ',
        &[
            ('カ', "k"),
            ('ガ', "g"),
            ('キ', "k"),
            ('ギ', "g"),
            ('ク', "k"),
            ('グ', "g"),
            ('ケ', "k"),
            ('ゲ', "g"),
            ('コ', "k"),
            ('ゴ', "g"),
            ('サ', "s"),
            ('ザ', "z"),
            ('シ', "s"),
            ('ジ', "j"),
            ('ス', "s"),
            ('ズ', "z"),
            ('セ', "s"),
            ('ゼ', "z"),
            ('ソ', "s"),
            ('ゾ', "z"),
            ('タ', "t"),
            ('ダ', "t"),
            ('チ', "t"),
            ('ヂ', "t"),
            ('ツ', "t"),
            ('ヅ', "t"),
            ('テ', "t"),
            ('デ', "t"),
            ('ト', "t"),
            ('ド', "t"),
            ('ハ', "h"),
            ('バ', "b"),
            ('パ', "p"),
            ('ヒ', "h"),
            ('ビ', "b"),
            ('ピ', "p"),
            ('フ', "f"),
            ('ブ', "b"),
            ('プ', "p"),
            ('ヘ', "h"),
            ('ベ', "b"),
            ('ペ', "p"),
            ('ホ', "h"),
            ('ボ', "b"),
            ('ポ', "p"),
            ('ラ', "r"),
            ('リ', "r"),
            ('ル', "r"),
            ('レ', "r"),
            ('ロ', "r"),
            ('ン', "n"),
        ],
    ),
    (
        'ン',
        &[
            ('ア', "n'"),
            ('イ', "n'"),
            ('ウ', "n'"),
            ('エ', "n'"),
            ('オ', "n'"),
            ('バ', "m"),
            ('パ', "m"),
            ('ビ', "m"),
            ('ピ', "m"),
            ('ブ', "m"),
            ('プ', "m"),
            ('ベ', "m"),
            ('ペ', "m"),
            ('ボ', "m"),
            ('ポ', "m"),
            ('マ', "m"),
            ('ミ', "m"),
            ('ム', "m"),
            ('メ', "m"),
            ('モ', "m"),
            ('ヤ', "n'"),
            ('ユ', "n'"),
            ('ヨ', "n'"),
        ],
    ),
];

/// The small tsu characters of the final `/(ッ|っ)\B/g` pass, and what it
/// writes in their place.
pub(crate) static FINAL_SOKUON: (&[char], &str) = (&['ッ', 'っ'], "t");
