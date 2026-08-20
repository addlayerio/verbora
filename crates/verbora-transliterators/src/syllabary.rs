// The kana syllabary Verbora romanizes, written out and cited.
//
// This file is the crate's single source of truth. `build.rs` `include!`s it,
// derives the katakana half, the long-vowel forms and the lookup index from
// it, and writes the result to `$OUT_DIR/index.rs`; the crate `include!`s
// that. Nothing here was produced by observing another implementation, and
// nothing downstream adds a mora this list does not contain.
//
// # Where the readings come from
//
// **Modified Hepburn**, as codified in the *ALA-LC Romanization Tables:
// Japanese* (American Library Association / Library of Congress), which
// follows ANSI Z39.11-1972 *System for the Romanization of Japanese* and
// BS 4812:1972. That is the source for the gojūon, the yōon, `し` = `shi`,
// `ち` = `chi`, `つ` = `tsu`, `ふ` = `fu`, `じ`/`ぢ` = `ji`, `ず`/`づ` = `zu`,
// `ゐ` = `i`, `ゑ` = `e` and `を` = `o`.
//
// **The extended (gairaigo) syllables** are those of 内閣告示第二号
// 「外来語の表記」 (Cabinet of Japan, Notification No. 2 of 1991,
// *Gairaigo no hyōki*), 第1表 and 第2表 — the two tables that say which
// kana combinations Japanese writes foreign sounds with. Their romanizations
// are the Hepburn consonant of the base kana plus the vowel of the small
// kana, which is the construction those tables are built on.
//
// Five syllables are **Verbora additions** to that list, marked `[voiced]`
// below. Each is the voiced counterpart of a syllable the notification does
// list, romanized by the same construction: `ぐぃ`/`ぐぇ`/`ぐぉ` beside the
// listed `くぃ`/`くぇ`/`くぉ`, and `でぃ`/`でゅ` beside the listed
// `てぃ`/`てゅ`. Without them `ディズニー` romanizes as `deizunī` rather
// than `dizunī`, which is a worse answer than the one the notification's own
// construction gives.
//
// **The single kana whose reading comes from the Unicode Character Database**
// rather than from a romanization table are the four `KATAKANA LETTER V*`
// (U+30F7..U+30FA) and the two digraphs `ゟ` U+309F `HIRAGANA DIGRAPH YORI`
// and `ヿ` U+30FF `KATAKANA DIGRAPH KOTO`. Their character names *are* their
// readings, and for the two digraphs the reading agrees with their
// compatibility decompositions (`<vertical> 3088 308A` = `より` and
// `<square> 30B3 30C8` = `コト`) — a property the crate's own tests check
// rather than assert.
//
// # Hiragana only; the katakana half is derived
//
// The Unicode Standard §18.4 encodes Hiragana (U+3040..U+309F) and Katakana
// (U+30A0..U+30FF) as parallel repertoires for one syllabary, with the
// katakana form of a kana exactly `0x60` above the hiragana form throughout
// U+3041..U+3096. `build.rs` derives every katakana entry by that offset, and
// `katakana_is_the_hiragana_half_shifted_by_0x60` re-checks the derivation
// against the generated index. Writing the katakana half out by hand would
// be 200 more lines whose only content is that same offset, and one typo in
// it would be a mora that romanizes differently depending on which script it
// was written in.
//
// [`HIRAGANA_ONLY`] and [`KATAKANA_ONLY`] hold the entries at each end of
// the two blocks that have no counterpart under that offset.
//
// # What is deliberately not here
//
// Three characters carry a mora but no romanization of their own, because
// what they romanize as depends on their neighbour. They are named as
// constants ([`SOKUON`], [`NASAL`], [`PROLONGED_SOUND_MARK`]) and handled by
// the scanner, and `build.rs` refuses to build if any of them also appears as
// a syllabary key.

/// The sokuon (促音), the geminating mora: `っ` U+3063 and `ッ` U+30C3.
///
/// ALA-LC romanizes it by doubling the consonant of the following syllable,
/// except before `ch` where it is `t` (`まっちゃ` = `matcha`). It has no
/// reading of its own, so it is not a syllabary entry.
pub(crate) const SOKUON: [char; 2] = ['\u{3063}', '\u{30C3}'];

/// The syllabic nasal (撥音): `ん` U+3093 and `ン` U+30F3.
///
/// ALA-LC romanizes it `m` before `b`, `m` and `p`, `n'` before a vowel or
/// `y`, and `n` everywhere else — so its romanization is a function of the
/// following syllable, not of itself.
pub(crate) const NASAL: [char; 2] = ['\u{3093}', '\u{30F3}'];

/// The prolonged sound mark (長音符) `ー` U+30FC.
///
/// ALA-LC romanizes it as a macron over the preceding vowel. It has no vowel
/// of its own.
pub(crate) const PROLONGED_SOUND_MARK: char = '\u{30FC}';

/// Every hiragana mora, paired with its modified-Hepburn romanization.
///
/// Keys are one or two Unicode scalar values; `build.rs` rejects a longer one.
/// Order is irrelevant — the generated index is leftmost-longest, so a
/// two-scalar key always wins over the one-scalar key it starts with, whatever
/// order they are listed in.
// Read by `build.rs`, which `include!`s this file, and by the crate's own
// enumeration tests. The crate itself queries the generated index instead.
#[allow(dead_code)]
pub(crate) static HIRAGANA: &[(&str, &str)] = &[
    // ---- Gojūon: the five vowels ---------------------------------------
    ("あ", "a"),
    ("い", "i"),
    ("う", "u"),
    ("え", "e"),
    ("お", "o"),
    // ---- Gojūon: か行 / が行 --------------------------------------------
    ("か", "ka"),
    ("き", "ki"),
    ("く", "ku"),
    ("け", "ke"),
    ("こ", "ko"),
    ("が", "ga"),
    ("ぎ", "gi"),
    ("ぐ", "gu"),
    ("げ", "ge"),
    ("ご", "go"),
    // ---- Gojūon: さ行 / ざ行 --------------------------------------------
    // `し` is `shi` and `じ` is `ji` in Hepburn, not `si`/`zi`: the initials
    // are palatalized and Hepburn spells the sound, not the column.
    ("さ", "sa"),
    ("し", "shi"),
    ("す", "su"),
    ("せ", "se"),
    ("そ", "so"),
    ("ざ", "za"),
    ("じ", "ji"),
    ("ず", "zu"),
    ("ぜ", "ze"),
    ("ぞ", "zo"),
    // ---- Gojūon: た行 / だ行 --------------------------------------------
    // `ぢ` and `づ` merged with `じ` and `ず` in modern Japanese; ALA-LC
    // romanizes all four by sound, so the merger is visible in the output.
    ("た", "ta"),
    ("ち", "chi"),
    ("つ", "tsu"),
    ("て", "te"),
    ("と", "to"),
    ("だ", "da"),
    ("ぢ", "ji"),
    ("づ", "zu"),
    ("で", "de"),
    ("ど", "do"),
    // ---- Gojūon: な行 ---------------------------------------------------
    ("な", "na"),
    ("に", "ni"),
    ("ぬ", "nu"),
    ("ね", "ne"),
    ("の", "no"),
    // ---- Gojūon: は行 / ば行 / ぱ行 -------------------------------------
    // `ふ` is `fu`: the initial is a bilabial fricative, not `h` + `u`.
    ("は", "ha"),
    ("ひ", "hi"),
    ("ふ", "fu"),
    ("へ", "he"),
    ("ほ", "ho"),
    ("ば", "ba"),
    ("び", "bi"),
    ("ぶ", "bu"),
    ("べ", "be"),
    ("ぼ", "bo"),
    ("ぱ", "pa"),
    ("ぴ", "pi"),
    ("ぷ", "pu"),
    ("ぺ", "pe"),
    ("ぽ", "po"),
    // ---- Gojūon: ま行 ---------------------------------------------------
    ("ま", "ma"),
    ("み", "mi"),
    ("む", "mu"),
    ("め", "me"),
    ("も", "mo"),
    // ---- Gojūon: や行 ---------------------------------------------------
    ("や", "ya"),
    ("ゆ", "yu"),
    ("よ", "yo"),
    // ---- Gojūon: ら行 ---------------------------------------------------
    ("ら", "ra"),
    ("り", "ri"),
    ("る", "ru"),
    ("れ", "re"),
    ("ろ", "ro"),
    // ---- Gojūon: わ行, and the two archaic kana -------------------------
    // ALA-LC romanizes `ゐ` `i`, `ゑ` `e` and `を` `o` — by their modern
    // sound, not by the `wi`/`we`/`wo` their names preserve.
    ("わ", "wa"),
    ("ゐ", "i"),
    ("ゑ", "e"),
    ("を", "o"),
    // ---- `ゔ`: the voiced `う` ------------------------------------------
    // 「外来語の表記」第2表 lists `ヴ`; ALA-LC writes it `vu`.
    ("ゔ", "vu"),
    // ---- Small kana standing alone --------------------------------------
    // A small kana that begins no digraph is still a mora, and is romanized
    // by the same reading as its full-size form. Their Unicode names say so:
    // `HIRAGANA LETTER SMALL A`, `… SMALL YA`, `… SMALL KA`.
    ("ぁ", "a"),
    ("ぃ", "i"),
    ("ぅ", "u"),
    ("ぇ", "e"),
    ("ぉ", "o"),
    ("ゃ", "ya"),
    ("ゅ", "yu"),
    ("ょ", "yo"),
    ("ゎ", "wa"),
    ("ゕ", "ka"),
    ("ゖ", "ke"),
    // ---- Yōon: base + small ya/yu/yo ------------------------------------
    // ALA-LC: the i-column kana loses its `i` and takes `y` + the small
    // kana's vowel — except in the sh/j/ch columns, where the `y` is already
    // in the initial and is not written twice (`しゃ` is `sha`, not `shya`).
    ("きゃ", "kya"),
    ("きゅ", "kyu"),
    ("きょ", "kyo"),
    ("ぎゃ", "gya"),
    ("ぎゅ", "gyu"),
    ("ぎょ", "gyo"),
    ("しゃ", "sha"),
    ("しゅ", "shu"),
    ("しょ", "sho"),
    ("じゃ", "ja"),
    ("じゅ", "ju"),
    ("じょ", "jo"),
    ("ちゃ", "cha"),
    ("ちゅ", "chu"),
    ("ちょ", "cho"),
    ("ぢゃ", "ja"),
    ("ぢゅ", "ju"),
    ("ぢょ", "jo"),
    ("にゃ", "nya"),
    ("にゅ", "nyu"),
    ("にょ", "nyo"),
    ("ひゃ", "hya"),
    ("ひゅ", "hyu"),
    ("ひょ", "hyo"),
    ("びゃ", "bya"),
    ("びゅ", "byu"),
    ("びょ", "byo"),
    ("ぴゃ", "pya"),
    ("ぴゅ", "pyu"),
    ("ぴょ", "pyo"),
    ("みゃ", "mya"),
    ("みゅ", "myu"),
    ("みょ", "myo"),
    ("りゃ", "rya"),
    ("りゅ", "ryu"),
    ("りょ", "ryo"),
    // ---- Extended: 「外来語の表記」第1表 --------------------------------
    ("しぇ", "she"),
    ("じぇ", "je"),
    ("ちぇ", "che"),
    ("つぁ", "tsa"),
    ("つぇ", "tse"),
    ("つぉ", "tso"),
    ("てぃ", "ti"),
    ("ふぁ", "fa"),
    ("ふぃ", "fi"),
    ("ふぇ", "fe"),
    ("ふぉ", "fo"),
    // ---- Extended: 「外来語の表記」第2表 --------------------------------
    ("いぇ", "ye"),
    ("うぃ", "wi"),
    ("うぇ", "we"),
    ("うぉ", "wo"),
    ("くぁ", "kwa"),
    ("くぃ", "kwi"),
    ("くぇ", "kwe"),
    ("くぉ", "kwo"),
    ("つぃ", "tsi"),
    ("とぅ", "tu"),
    ("ぐぁ", "gwa"),
    ("どぅ", "du"),
    ("ゔぁ", "va"),
    ("ゔぃ", "vi"),
    ("ゔぇ", "ve"),
    ("ゔぉ", "vo"),
    ("てゅ", "tyu"),
    ("ふゅ", "fyu"),
    ("ゔゅ", "vyu"),
    // ---- Extended: the voiced counterparts Verbora adds -----------------
    // `[voiced]` — each mirrors a syllable 第2表 does list, with the same
    // construction applied to the voiced base kana. See this file's header.
    ("ぐぃ", "gwi"), // [voiced] beside くぃ kwi
    ("ぐぇ", "gwe"), // [voiced] beside くぇ kwe
    ("ぐぉ", "gwo"), // [voiced] beside くぉ kwo
    ("でぃ", "di"),  // [voiced] beside てぃ ti
    ("でゅ", "dyu"), // [voiced] beside てゅ tyu
];

/// The hiragana entries with no katakana counterpart at `hiragana + 0x60`.
///
/// U+309F is the last code point of the Hiragana block, so `+ 0x60` lands on
/// U+30FF `ヿ` — a different character with a different reading, not the
/// katakana form of this one. It is therefore excluded from the derivation and
/// listed here.
// Read by `build.rs`, which `include!`s this file, and by the crate's own
// enumeration tests. The crate itself queries the generated index instead.
#[allow(dead_code)]
pub(crate) static HIRAGANA_ONLY: &[(&str, &str)] = &[
    // U+309F `HIRAGANA DIGRAPH YORI`, compatibility decomposition
    // `<vertical> 3088 308A` = `より`.
    ("ゟ", "yori"),
];

/// The katakana entries with no hiragana counterpart at `hiragana + 0x60`.
///
/// Everything else in katakana is derived from [`HIRAGANA`] by that offset;
/// these six characters exist only in the Katakana block.
// Read by `build.rs`, which `include!`s this file, and by the crate's own
// enumeration tests. The crate itself queries the generated index instead.
#[allow(dead_code)]
pub(crate) static KATAKANA_ONLY: &[(&str, &str)] = &[
    // U+30F7..U+30FA, named `KATAKANA LETTER VA`, `… VI`, `… VE`, `… VO` in
    // the UCD. They are the `ワ`-row with a voiced sound mark and they
    // romanize as the `ヴァ`/`ヴィ`/`ヴェ`/`ヴォ` above do.
    ("ヷ", "va"),
    ("ヸ", "vi"),
    ("ヹ", "ve"),
    ("ヺ", "vo"),
    // U+30FF `KATAKANA DIGRAPH KOTO`, compatibility decomposition
    // `<square> 30B3 30C8` = `コト`.
    ("ヿ", "koto"),
    // U+30FB `KATAKANA MIDDLE DOT`, `General_Category = Po`. It is
    // punctuation rather than a mora: it separates the elements of a
    // transcribed foreign name (`ボージョレー・ヌーヴォー`). Verbora
    // romanizes it as one ASCII space, which is the separator the romanized
    // text already uses between words. This is a Verbora decision, not a
    // clause of ALA-LC.
    ("・", " "),
];
