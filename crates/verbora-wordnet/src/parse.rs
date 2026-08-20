//! Strict readers for the fixed-shape numeric fields of the WordNet database
//! format.
//!
//! Every numeric field in `wndb(5WN)` is a run of ASCII digits in a stated
//! radix — decimal everywhere except `w_cnt`, `lex_id` and the `source/target`
//! word numbers, which are hexadecimal. These readers accept exactly that and
//! nothing else: no sign, no whitespace, no trailing text, no partial parse.
//! A field that does not match is a malformed record, reported as such, rather
//! than a value silently truncated at the first character that did not fit.

use std::num::NonZeroU8;

use crate::error::RecordError;
use crate::pointer::PointerScope;
use crate::synset::SynsetOffset;

type FieldResult<T> = std::result::Result<T, RecordError>;

/// The next token, or [`RecordError::MissingField`].
pub(crate) fn required<'a>(token: Option<&'a str>, field: &'static str) -> FieldResult<&'a str> {
    token.ok_or(RecordError::MissingField { field })
}

fn invalid(field: &'static str, value: &str) -> RecordError {
    RecordError::InvalidField {
        field,
        value: value.to_owned(),
    }
}

/// An unsigned decimal integer, with no sign, prefix or trailing text.
pub(crate) fn decimal_u32(field: &'static str, token: &str) -> FieldResult<u32> {
    if token.is_empty() || !token.bytes().all(|b| b.is_ascii_digit()) {
        return Err(invalid(field, token));
    }
    token.parse::<u32>().map_err(|_| invalid(field, token))
}

/// An unsigned decimal integer that must fit in a byte.
pub(crate) fn decimal_u8(field: &'static str, token: &str) -> FieldResult<u8> {
    let v = decimal_u32(field, token)?;
    u8::try_from(v).map_err(|_| invalid(field, token))
}

/// An unsigned hexadecimal integer that must fit in a byte.
///
/// `wndb(5WN)` writes `w_cnt` as two hexadecimal digits and `lex_id` as one, so
/// `0b` is eleven. Reading either as decimal would truncate every synset with
/// ten or more words and shift the position of `p_cnt` behind it.
pub(crate) fn hex_u8(field: &'static str, token: &str) -> FieldResult<u8> {
    if token.is_empty() || !token.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(invalid(field, token));
    }
    u8::from_str_radix(token, 16).map_err(|_| invalid(field, token))
}

/// A `synset_offset`: an eight-digit, zero-filled decimal byte position.
///
/// The width is not enforced, because the same field appears unpadded nowhere
/// in the format but the value is what matters; what *is* enforced is that the
/// token is decimal digits only.
pub(crate) fn offset(field: &'static str, token: &str) -> FieldResult<SynsetOffset> {
    Ok(SynsetOffset::new(decimal_u32(field, token)?))
}

/// The `source/target` field: four hexadecimal digits, two per word number.
///
/// `0000` is the semantic reading. Anything else must have **both** halves
/// non-zero: `wndb(5WN)` defines no meaning for a pointer that names a word in
/// one synset and no word in the other, so a half-zero field is malformed
/// rather than quietly reinterpreted.
pub(crate) fn pointer_scope(token: &str) -> FieldResult<PointerScope> {
    const FIELD: &str = "source/target";
    if token.len() != 4 || !token.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(invalid(FIELD, token));
    }
    let source = u8::from_str_radix(&token[..2], 16).map_err(|_| invalid(FIELD, token))?;
    let target = u8::from_str_radix(&token[2..], 16).map_err(|_| invalid(FIELD, token))?;
    match (NonZeroU8::new(source), NonZeroU8::new(target)) {
        (None, None) => Ok(PointerScope::Semantic),
        (Some(source_word), Some(target_word)) => Ok(PointerScope::Lexical {
            source_word,
            target_word,
        }),
        _ => Err(invalid(FIELD, token)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decimal_fields_reject_everything_that_is_not_digits() {
        assert_eq!(decimal_u32("x", "0").unwrap(), 0);
        assert_eq!(decimal_u32("x", "08515608").unwrap(), 8_515_608);
        for bad in [
            "",
            " 1",
            "1 ",
            "+1",
            "-1",
            "1a",
            "a",
            "0x10",
            "1.0",
            "١٢",
            "4294967296",
        ] {
            assert!(decimal_u32("x", bad).is_err(), "{bad:?}");
        }
        assert!(decimal_u8("x", "256").is_err());
        assert_eq!(decimal_u8("x", "255").unwrap(), 255);
    }

    /// The hexadecimal reading is the whole reason `w_cnt` needs its own
    /// reader: `0b` is eleven words, and a decimal reading would see zero.
    #[test]
    fn hexadecimal_fields_read_all_two_hundred_and_fifty_six_byte_values() {
        for v in 0u8..=255 {
            let lower = format!("{v:02x}");
            let upper = format!("{v:02X}");
            assert_eq!(hex_u8("w_cnt", &lower).unwrap(), v, "{lower}");
            assert_eq!(hex_u8("w_cnt", &upper).unwrap(), v, "{upper}");
        }
        assert_eq!(hex_u8("w_cnt", "0b").unwrap(), 11);
        assert_eq!(hex_u8("w_cnt", "3").unwrap(), 3);
        for bad in ["", "g", "0x0b", " 0b", "100", "-1"] {
            assert!(hex_u8("w_cnt", bad).is_err(), "{bad:?}");
        }
    }

    /// Enumerates all 65 536 four-hex-digit fields: exactly one is semantic,
    /// exactly 255 × 255 are lexical, and every half-zero field is refused.
    #[test]
    fn every_four_digit_source_target_field_classifies_the_same_way() {
        let mut semantic = 0usize;
        let mut lexical = 0usize;
        let mut refused = 0usize;
        for raw in 0u32..=0xFFFF {
            let token = format!("{raw:04x}");
            match pointer_scope(&token) {
                Ok(PointerScope::Semantic) => {
                    assert_eq!(raw, 0);
                    semantic += 1;
                }
                Ok(PointerScope::Lexical {
                    source_word,
                    target_word,
                }) => {
                    assert_eq!(u32::from(source_word.get()), raw >> 8);
                    assert_eq!(u32::from(target_word.get()), raw & 0xFF);
                    lexical += 1;
                }
                Err(_) => refused += 1,
            }
        }
        assert_eq!(semantic, 1);
        assert_eq!(lexical, 255 * 255);
        // 255 fields with a zero source and 255 with a zero target.
        assert_eq!(refused, 255 + 255);
        assert_eq!(semantic + lexical + refused, 0x1_0000);
    }

    #[test]
    fn malformed_source_target_fields_are_refused() {
        for bad in ["", "0", "000", "00000", "0g00", "00 0"] {
            assert!(pointer_scope(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn a_missing_token_names_the_field() {
        assert_eq!(
            required(None, "p_cnt").unwrap_err(),
            RecordError::MissingField { field: "p_cnt" }
        );
        assert_eq!(required(Some("3"), "p_cnt").unwrap(), "3");
    }

    #[test]
    fn offsets_are_read_as_decimal_not_octal() {
        // A leading zero is padding, never a radix marker.
        assert_eq!(offset("synset_offset", "00000083").unwrap().get(), 83);
        assert_eq!(offset("synset_offset", "0").unwrap().get(), 0);
        assert!(offset("synset_offset", "0x10").is_err());
    }
}
