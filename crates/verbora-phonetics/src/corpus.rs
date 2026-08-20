//! A non-ASCII name corpus, for the crate-wide enumeration tests.
//!
//! The 653-name list in `benches/data/names.json` — the corpus this crate's
//! benchmarks and its earlier correctness sweeps used — is **entirely ASCII**.
//! An ASCII corpus is invariant under every text-unit rule anyone would
//! propose, so it cannot detect a text-unit change at all: it is exactly the
//! shape of coverage that let a UTF-16 code-unit reading of `token[i]` sit
//! unnoticed underneath these encoders. This module supplies the corpus that
//! can.
//!
//! Entries are real personal and place names, not random scalars, so a failure
//! here is a failure on input a caller will actually hand the library. The
//! last group is deliberately pathological: it holds the Unicode shapes that
//! break naive indexing — astral scalars, combining marks, case mappings that
//! change length, a Turkish dotted capital I, a Greek final sigma, a
//! zero-width joiner, a byte-order mark and a non-breaking space.

/// Real names, in the scripts a name index actually meets.
pub(crate) const NON_ASCII_NAMES: &[&str] = &[
    // Latin with diacritics — French, German, Spanish, Portuguese, Polish,
    // Czech, Hungarian, Turkish, Nordic, Vietnamese.
    "Müller",
    "Schröder",
    "Weiß",
    "Gößmann",
    "Éric",
    "Renée",
    "Françoise",
    "Bénédicte",
    "Léa",
    "José",
    "Muñoz",
    "Peña",
    "Ibáñez",
    "Gonçalves",
    "Conceição",
    "Łukasz",
    "Wałęsa",
    "Kraków",
    "Żółkiewski",
    "Dvořák",
    "Škoda",
    "Čapek",
    "Kőszeg",
    "Örkény",
    "Gülşen",
    "Işık",
    "İstanbul",
    "Åkerman",
    "Ærøskøbing",
    "Sørensen",
    "Þorláksson",
    "Guðmundsdóttir",
    "Nguyễn",
    "Trần",
    "Đặng",
    // Cyrillic.
    "Москва",
    "Достоевский",
    "Чайковский",
    "Толстой",
    "Мельник",
    "Шевченко",
    "Ђорђевић",
    "Јовановић",
    // Greek.
    "Παπαδόπουλος",
    "Αθήνα",
    "Ελευθέριος",
    "Καραμανλής",
    "ΟΔΥΣΣΕΥΣ",
    // Hebrew.
    "ירושלים",
    "כהן",
    "לוי",
    "בן־גוריון",
    // Arabic.
    "محمد",
    "القاهرة",
    "عبد الرحمن",
    // Devanagari, Bengali, Tamil.
    "नई दिल्ली",
    "गांधी",
    "রবীন্দ্রনাথ",
    "சென்னை",
    // Han, kana, hangul.
    "日本語",
    "東京",
    "北京",
    "王小明",
    "さくら",
    "ヤマモト",
    "서울",
    "김민준",
    // Thai, Georgian, Armenian, Amharic.
    "กรุงเทพมหานคร",
    "თბილისი",
    "Երևան",
    "አዲስ አበባ",
    // Mixed-script and multi-part names, the shape a real index sees.
    "Van der Berg",
    "Mac Gregor",
    "O'Brien",
    "Jean-Luc Picard",
    "de la Cruz",
    "Ho Chi Minh",
    "Владимир Ivanov",
    "François-Xavier",
];

/// Unicode shapes chosen because they break naive text handling.
///
/// Each entry names the trap it sets in the comment beside it. None is a
/// realistic name; that is the point — an encoder must not panic or emit
/// something the input never implied, however the input is spelled.
pub(crate) const PATHOLOGICAL: &[&str] = &[
    "",          // no scalars at all
    " ",         // whitespace only
    "\u{00A0}",  // NBSP: whitespace to Unicode, not to ASCII
    "\u{FEFF}",  // byte-order mark
    "\u{200D}",  // zero-width joiner
    "\u{0301}",  // a lone combining acute, with nothing to combine with
    "A\u{0301}", // decomposed Á: two scalars, one grapheme
    "\u{00C1}",  // precomposed Á: one scalar
    "ß",         // uppercases to two characters
    "ẞ",         // ... and its capital, which uppercases to itself
    "ﬁ",         // ligature: uppercases to FI
    "ﬃ",         // ... and a three-character expansion
    "İ",         // Turkish dotted capital I: lowercases to two scalars
    "ı",         // dotless i
    "ΑΣ",        // final sigma: context-dependent lowercase
    "ας",
    "\u{212A}",           // Kelvin sign: lowercases to plain k
    "😀",                 // astral scalar
    "a😀b",               // astral scalar between letters
    "𝓢𝓶𝓲𝓽𝓱",              // astral mathematical script: "Smith"
    "\u{10FFFF}",         // the last valid scalar
    "\u{1F1FA}\u{1F1F8}", // a regional-indicator pair
    "12345",              // digits only
    "...",                // punctuation only
    "--&',.",             // exactly the characters Match Rating strips
    "a-b",                // an interior hyphen
    "  spaces  ",         // leading and trailing whitespace
];
