//! Case-insensitive character sets, tested one scalar at a time.
//!
//! Checked-in data with no generator in the tree, so the sets are unverifiable
//! as they stand. Grounding them in Carry's published description is an open
//! item — see `docs/design/rust-native-migration.md`.

/// Carry's vowel set. `y` is deliberately **not** a vowel here.
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

/// The Spanish vowel set.
///
/// It is case-insensitive, which is why an uppercase word still gets its regions
/// marked — while every suffix comparison stays case-sensitive, so it then
/// matches nothing.
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
