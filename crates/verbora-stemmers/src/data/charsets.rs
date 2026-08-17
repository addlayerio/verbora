//! Case-insensitive character sets applied per character.
//!
//! Generated data; see the note in `gen-stemmer-data`.

/// Carry's vowel set, from `defaultConf.vowels`. Note `y` is NOT a vowel here.
#[inline]
pub(crate) fn is_carry_vowel(unit: u16) -> bool {
    matches!(unit,
        65
            | 69
            | 73
            | 79
            | 85
            | 97
            | 101
            | 105
            | 111
            | 117
            | 192
            | 194
            | 196
            | 200..=203
            | 206..=207
            | 212
            | 214
            | 217
            | 219..=220
            | 224
            | 226
            | 228
            | 232..=235
            | 238..=239
            | 244
            | 246
            | 249
            | 251..=252
            | 338..=339
    )
}

/// PorterStemmerEs.isVowel. The /i flag is why an uppercase word still gets regions marked — while every suffix comparison stays case-sensitive, so it matches nothing.
#[inline]
pub(crate) fn is_es_vowel(unit: u16) -> bool {
    matches!(
        unit,
        65 | 69
            | 73
            | 79
            | 85
            | 97
            | 101
            | 105
            | 111
            | 117
            | 193
            | 201
            | 205
            | 211
            | 218
            | 225
            | 233
            | 237
            | 243
            | 250
    )
}
