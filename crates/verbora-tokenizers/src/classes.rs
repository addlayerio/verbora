//! Character classes, machine-derived from the reference patterns.
//!
//! DO NOT EDIT BY HAND. Every range below was produced by enumerating the whole
//! Basic Multilingual Plane against the source patterns rather than transcribing
//! them, because transcribing is how a port acquires silent divergences: a
//! case-insensitive class admits uppercase forms whose membership depends on
//! canonicalisation rules that do *not* fold non-ASCII onto ASCII, and Latin-1
//! ranges such as `À-ÿ` quietly contain the math symbols × and ÷.
//!
//! Astral code points are always separators: the source patterns are not
//! Unicode-aware, so they see a surrogate *pair*, and neither half is ever a
//! word character. Only the BMP therefore needed enumerating.

/// Whether `c` is a word character for AggressiveTokenizer (English).
///
/// Derived from `/[^a-zA-Z0-9'\-/]/` by enumerating the Basic Multilingual Plane
/// (5 ranges, 65 code points). Astral characters are never word
/// characters: the source regex has no `u` flag, so it only ever sees the two
/// surrogate halves, and neither is in the class.
#[inline]
pub const fn is_word_en(c: char) -> bool {
    matches!(c,
        '\u{27}'
        | '\u{2d}'
        | '\u{2f}'..='\u{39}'
        | '\u{41}'..='\u{5a}'
        | '\u{61}'..='\u{7a}'
    )
}

/// Whether `c` is a word character for AggressiveTokenizerNl.
///
/// Derived from `/[^a-zA-Z0-9_'-]/` by enumerating the Basic Multilingual Plane
/// (6 ranges, 65 code points). Astral characters are never word
/// characters: the source regex has no `u` flag, so it only ever sees the two
/// surrogate halves, and neither is in the class.
#[inline]
pub const fn is_word_nl(c: char) -> bool {
    matches!(c,
        '\u{27}'
        | '\u{2d}'
        | '\u{30}'..='\u{39}'
        | '\u{41}'..='\u{5a}'
        | '\u{5f}'
        | '\u{61}'..='\u{7a}'
    )
}

/// Whether `c` is a word character for AggressiveTokenizerDe.
///
/// Derived from `/[^a-zA-Z0-9ßäöü_'-]/` by enumerating the Basic Multilingual Plane
/// (10 ranges, 69 code points). Astral characters are never word
/// characters: the source regex has no `u` flag, so it only ever sees the two
/// surrogate halves, and neither is in the class.
#[inline]
pub const fn is_word_de(c: char) -> bool {
    matches!(c,
        '\u{27}'
        | '\u{2d}'
        | '\u{30}'..='\u{39}'
        | '\u{41}'..='\u{5a}'
        | '\u{5f}'
        | '\u{61}'..='\u{7a}'
        | '\u{df}'
        | '\u{e4}'
        | '\u{f6}'
        | '\u{fc}'
    )
}

/// Whether `c` is a word character for AggressiveTokenizerFr.
///
/// Derived from `/[^a-z0-9äâàéèëêïîöôùüûœç-]/i` by enumerating the Basic Multilingual Plane
/// (23 ranges, 95 code points). Astral characters are never word
/// characters: the source regex has no `u` flag, so it only ever sees the two
/// surrogate halves, and neither is in the class.
#[inline]
pub const fn is_word_fr(c: char) -> bool {
    matches!(c,
        '\u{2d}'
        | '\u{30}'..='\u{39}'
        | '\u{41}'..='\u{5a}'
        | '\u{61}'..='\u{7a}'
        | '\u{c0}'
        | '\u{c2}'
        | '\u{c4}'
        | '\u{c7}'..='\u{cb}'
        | '\u{ce}'..='\u{cf}'
        | '\u{d4}'
        | '\u{d6}'
        | '\u{d9}'
        | '\u{db}'..='\u{dc}'
        | '\u{e0}'
        | '\u{e2}'
        | '\u{e4}'
        | '\u{e7}'..='\u{eb}'
        | '\u{ee}'..='\u{ef}'
        | '\u{f4}'
        | '\u{f6}'
        | '\u{f9}'
        | '\u{fb}'..='\u{fc}'
        | '\u{152}'..='\u{153}'
    )
}

/// Whether `c` is a word character for AggressiveTokenizerEs.
///
/// Derived from `/[^a-zA-Zá-úÁ-ÚñÑüÜ]/` by enumerating the Basic Multilingual Plane
/// (6 ranges, 106 code points). Astral characters are never word
/// characters: the source regex has no `u` flag, so it only ever sees the two
/// surrogate halves, and neither is in the class.
#[inline]
pub const fn is_word_es(c: char) -> bool {
    matches!(c,
        '\u{41}'..='\u{5a}'
        | '\u{61}'..='\u{7a}'
        | '\u{c1}'..='\u{da}'
        | '\u{dc}'
        | '\u{e1}'..='\u{fa}'
        | '\u{fc}'
    )
}

/// Whether `c` is a word character for AggressiveTokenizerIt.
///
/// Derived from `/\W/` by enumerating the Basic Multilingual Plane
/// (4 ranges, 63 code points). Astral characters are never word
/// characters: the source regex has no `u` flag, so it only ever sees the two
/// surrogate halves, and neither is in the class.
#[inline]
pub const fn is_word_it(c: char) -> bool {
    matches!(c,
        '\u{30}'..='\u{39}'
        | '\u{41}'..='\u{5a}'
        | '\u{5f}'
        | '\u{61}'..='\u{7a}'
    )
}

/// Whether `c` is a word character for AggressiveTokenizerPt.
///
/// Derived from `/[^a-zA-Zà-úÀ-Ú]/` by enumerating the Basic Multilingual Plane
/// (4 ranges, 106 code points). Astral characters are never word
/// characters: the source regex has no `u` flag, so it only ever sees the two
/// surrogate halves, and neither is in the class.
#[inline]
pub const fn is_word_pt(c: char) -> bool {
    matches!(c,
        '\u{41}'..='\u{5a}'
        | '\u{61}'..='\u{7a}'
        | '\u{c0}'..='\u{da}'
        | '\u{e0}'..='\u{fa}'
    )
}

/// Whether `c` is a word character for AggressiveTokenizerVi.
///
/// Derived from `/[^a-z0-9àáảãạăắằẳẵặâấầẩẫậéèẻẽẹêếềểễệíìỉĩịóòỏõọôốồổỗộơớờởỡợúùủũụưứừửữựýỳỷỹỵđ]/i` by enumerating the Basic Multilingual Plane
/// (22 ranges, 196 code points). Astral characters are never word
/// characters: the source regex has no `u` flag, so it only ever sees the two
/// surrogate halves, and neither is in the class.
#[inline]
pub const fn is_word_vi(c: char) -> bool {
    matches!(c,
        '\u{30}'..='\u{39}'
        | '\u{41}'..='\u{5a}'
        | '\u{61}'..='\u{7a}'
        | '\u{c0}'..='\u{c3}'
        | '\u{c8}'..='\u{ca}'
        | '\u{cc}'..='\u{cd}'
        | '\u{d2}'..='\u{d5}'
        | '\u{d9}'..='\u{da}'
        | '\u{dd}'
        | '\u{e0}'..='\u{e3}'
        | '\u{e8}'..='\u{ea}'
        | '\u{ec}'..='\u{ed}'
        | '\u{f2}'..='\u{f5}'
        | '\u{f9}'..='\u{fa}'
        | '\u{fd}'
        | '\u{102}'..='\u{103}'
        | '\u{110}'..='\u{111}'
        | '\u{128}'..='\u{129}'
        | '\u{168}'..='\u{169}'
        | '\u{1a0}'..='\u{1a1}'
        | '\u{1af}'..='\u{1b0}'
        | '\u{1ea0}'..='\u{1ef9}'
    )
}

/// Whether `c` is a word character for AggressiveTokenizerRu.
///
/// Derived from `/[^a-zа-яё0-9]/i` by enumerating the Basic Multilingual Plane
/// (7 ranges, 135 code points). Astral characters are never word
/// characters: the source regex has no `u` flag, so it only ever sees the two
/// surrogate halves, and neither is in the class.
#[inline]
pub const fn is_word_ru(c: char) -> bool {
    matches!(c,
        '\u{30}'..='\u{39}'
        | '\u{41}'..='\u{5a}'
        | '\u{61}'..='\u{7a}'
        | '\u{401}'
        | '\u{410}'..='\u{44f}'
        | '\u{451}'
        | '\u{1c80}'..='\u{1c86}'
    )
}

/// Whether `c` is a word character for AggressiveTokenizerUk.
///
/// Derived from `/[^a-zа-яґєії0-9]/i` by enumerating the Basic Multilingual Plane
/// (10 ranges, 141 code points). Astral characters are never word
/// characters: the source regex has no `u` flag, so it only ever sees the two
/// surrogate halves, and neither is in the class.
#[inline]
pub const fn is_word_uk(c: char) -> bool {
    matches!(c,
        '\u{30}'..='\u{39}'
        | '\u{41}'..='\u{5a}'
        | '\u{61}'..='\u{7a}'
        | '\u{404}'
        | '\u{406}'..='\u{407}'
        | '\u{410}'..='\u{44f}'
        | '\u{454}'
        | '\u{456}'..='\u{457}'
        | '\u{490}'..='\u{491}'
        | '\u{1c80}'..='\u{1c86}'
    )
}

/// Whether `c` is a word character for AggressiveTokenizerPl.
///
/// Derived from `/[^a-zążśźęćńół0-9]/i` by enumerating the Basic Multilingual Plane
/// (10 ranges, 80 code points). Astral characters are never word
/// characters: the source regex has no `u` flag, so it only ever sees the two
/// surrogate halves, and neither is in the class.
#[inline]
pub const fn is_word_pl(c: char) -> bool {
    matches!(c,
        '\u{30}'..='\u{39}'
        | '\u{41}'..='\u{5a}'
        | '\u{61}'..='\u{7a}'
        | '\u{d3}'
        | '\u{f3}'
        | '\u{104}'..='\u{107}'
        | '\u{118}'..='\u{119}'
        | '\u{141}'..='\u{144}'
        | '\u{15a}'..='\u{15b}'
        | '\u{179}'..='\u{17c}'
    )
}

/// Whether `c` is a word character for AggressiveTokenizerId.
///
/// Derived from `/[^a-z0-9-]/` by enumerating the Basic Multilingual Plane
/// (3 ranges, 37 code points). Astral characters are never word
/// characters: the source regex has no `u` flag, so it only ever sees the two
/// surrogate halves, and neither is in the class.
#[inline]
pub const fn is_word_id(c: char) -> bool {
    matches!(c,
        '\u{2d}'
        | '\u{30}'..='\u{39}'
        | '\u{61}'..='\u{7a}'
    )
}

/// Whether `c` is a word character for AggressiveTokenizerNo (post-diacritic-removal).
///
/// Derived from `/[^A-Za-z0-9_æøåÆØÅäÄöÖüÜ]/` by enumerating the Basic Multilingual Plane
/// (12 ranges, 75 code points). Astral characters are never word
/// characters: the source regex has no `u` flag, so it only ever sees the two
/// surrogate halves, and neither is in the class.
#[inline]
pub const fn is_word_no(c: char) -> bool {
    matches!(c,
        '\u{30}'..='\u{39}'
        | '\u{41}'..='\u{5a}'
        | '\u{5f}'
        | '\u{61}'..='\u{7a}'
        | '\u{c4}'..='\u{c6}'
        | '\u{d6}'
        | '\u{d8}'
        | '\u{dc}'
        | '\u{e4}'..='\u{e6}'
        | '\u{f6}'
        | '\u{f8}'
        | '\u{fc}'
    )
}

/// Whether `c` is a word character for AggressiveTokenizerSv (post-diacritic-removal).
///
/// Derived from `/[^A-Za-z0-9_åÅäÄöÖüÜ-]/` by enumerating the Basic Multilingual Plane
/// (11 ranges, 72 code points). Astral characters are never word
/// characters: the source regex has no `u` flag, so it only ever sees the two
/// surrogate halves, and neither is in the class.
#[inline]
pub const fn is_word_sv(c: char) -> bool {
    matches!(c,
        '\u{2d}'
        | '\u{30}'..='\u{39}'
        | '\u{41}'..='\u{5a}'
        | '\u{5f}'
        | '\u{61}'..='\u{7a}'
        | '\u{c4}'..='\u{c5}'
        | '\u{d6}'
        | '\u{dc}'
        | '\u{e4}'..='\u{e5}'
        | '\u{f6}'
        | '\u{fc}'
    )
}

/// Whether `c` is a word character for WordTokenizer.
///
/// Derived from `/[^A-Za-zА-Яа-я0-9_]/` by enumerating the Basic Multilingual Plane
/// (5 ranges, 127 code points). Astral characters are never word
/// characters: the source regex has no `u` flag, so it only ever sees the two
/// surrogate halves, and neither is in the class.
#[inline]
pub const fn is_word_word(c: char) -> bool {
    matches!(c,
        '\u{30}'..='\u{39}'
        | '\u{41}'..='\u{5a}'
        | '\u{5f}'
        | '\u{61}'..='\u{7a}'
        | '\u{410}'..='\u{44f}'
    )
}

/// Whether `c` is a word character for OrthographyTokenizer, language "fi".
///
/// Derived from `/[^A-Za-zÅåÄäÖö]/` by enumerating the Basic Multilingual Plane
/// (6 ranges, 58 code points). Astral characters are never word
/// characters: the source regex has no `u` flag, so it only ever sees the two
/// surrogate halves, and neither is in the class.
#[inline]
pub const fn is_word_fi(c: char) -> bool {
    matches!(c,
        '\u{41}'..='\u{5a}'
        | '\u{61}'..='\u{7a}'
        | '\u{c4}'..='\u{c5}'
        | '\u{d6}'
        | '\u{e4}'..='\u{e5}'
        | '\u{f6}'
    )
}

/// Whether `c` is a word character for WordPunctTokenizer alternative 1.
///
/// Derived from `/[^A-Za-zÀ-ÿ-]/i` by enumerating the Basic Multilingual Plane
/// (5 ranges, 118 code points). Astral characters are never word
/// characters: the source regex has no `u` flag, so it only ever sees the two
/// surrogate halves, and neither is in the class.
#[inline]
pub const fn is_word_wordpunct_letter(c: char) -> bool {
    matches!(c,
        '\u{2d}'
        | '\u{41}'..='\u{5a}'
        | '\u{61}'..='\u{7a}'
        | '\u{c0}'..='\u{ff}'
        | '\u{178}'
    )
}

/// Whether `c` is a word character for WordPunctTokenizer alternative 2.
///
/// Derived from `/[^0-9._]/` by enumerating the Basic Multilingual Plane
/// (3 ranges, 12 code points). Astral characters are never word
/// characters: the source regex has no `u` flag, so it only ever sees the two
/// surrogate halves, and neither is in the class.
#[inline]
pub const fn is_word_wordpunct_digit(c: char) -> bool {
    matches!(c, '\u{2e}' | '\u{30}'..='\u{39}' | '\u{5f}')
}

/// Whether `c` is a word character for TreebankWordTokenizer punctuation-padding keep set.
///
/// Derived from `/[^\w.'\-/+<>,&]/` by enumerating the Basic Multilingual Plane
/// (7 ranges, 72 code points). Astral characters are never word
/// characters: the source regex has no `u` flag, so it only ever sees the two
/// surrogate halves, and neither is in the class.
#[inline]
pub const fn is_word_treebank_keep(c: char) -> bool {
    matches!(c,
        '\u{26}'..='\u{27}'
        | '\u{2b}'..='\u{39}'
        | '\u{3c}'
        | '\u{3e}'
        | '\u{41}'..='\u{5a}'
        | '\u{5f}'
        | '\u{61}'..='\u{7a}'
    )
}
