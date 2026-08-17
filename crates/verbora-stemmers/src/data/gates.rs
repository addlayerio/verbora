//! The per-language gate predicates from the reference's `stemmer_*` modules.
//!
//! Generated data: each class was swept over the entire Basic Multilingual Plane
//! and the accepting code units collapsed into ranges, so the `/i` flag's
//! surprises are captured rather than guessed at.

/// German — note this is the SPANISH class, a copy/paste artefact in stemmer_de: it has no ä/ö/ü/ß.
#[inline]
pub(crate) fn gate_de(unit: u16) -> bool {
    matches!(unit,
        48..=57
            |         65..=90
            |         97..=122
            |         193
            |         201
            |         205
            |         209
            |         211
            |         218
            |         220
            |         225
            |         233
            |         237
            |         241
            |         243
            |         250
            |         252
    )
}

/// Spanish.
#[inline]
pub(crate) fn gate_es(unit: u16) -> bool {
    matches!(unit,
        48..=57
            |         65..=90
            |         97..=122
            |         193
            |         201
            |         205
            |         209
            |         211
            |         218
            |         220
            |         225
            |         233
            |         237
            |         241
            |         243
            |         250
            |         252
    )
}

/// French, shared by PorterStemmerFr and CarryStemmerFr; omits ä ö ü œ æ.
#[inline]
pub(crate) fn gate_fr(unit: u16) -> bool {
    matches!(unit,
        48..=57
            |         65..=90
            |         97..=122
            |         192
            |         194
            |         199..=203
            |         206..=207
            |         212
            |         217
            |         219
            |         224
            |         226
            |         231..=235
            |         238..=239
            |         244
            |         249
            |         251
    )
}

/// Italian.
#[inline]
pub(crate) fn gate_it(unit: u16) -> bool {
    matches!(unit,
        48..=57
            |         65..=90
            |         97..=122
            |         192
            |         200
            |         204
            |         210
            |         217
            |         224
            |         232
            |         236
            |         242
            |         249
    )
}

/// Dutch.
#[inline]
pub(crate) fn gate_nl(unit: u16) -> bool {
    matches!(unit,
        48..=57
            |         65..=90
            |         97..=122
            |         193
            |         196
            |         200..=201
            |         203
            |         205
            |         207
            |         211
            |         214
            |         218
            |         220
            |         225
            |         228
            |         232..=233
            |         235
            |         237
            |         239
            |         243
            |         246
            |         250
            |         252
    )
}

/// Russian.
#[inline]
pub(crate) fn gate_ru(unit: u16) -> bool {
    matches!(unit,
        48..=57
            |         1025
            |         1040..=1103
            |         1105
            |         7296..=7302
    )
}

/// Ukrainian.
#[inline]
pub(crate) fn gate_uk(unit: u16) -> bool {
    matches!(unit,
        48..=57
            |         1028
            |         1030..=1031
            |         1040..=1103
            |         1108
            |         1110..=1111
            |         1168..=1169
            |         7296..=7302
    )
}
