//! The whitespace class this crate splits and trims on.
//!
//! Private, and deliberately local: it used to live in `verbora-core`, where it
//! was the only item this crate imported from that crate and the only item any
//! crate imported from that module. A definition one crate uses belongs in that
//! crate.

/// Whether `c` is whitespace for the two places this crate splits or trims on
/// it: the class label a [`Classifier`](crate::Classifier) is trained with, and
/// the line and token boundaries of a Brown-format corpus.
///
/// # The exact set
///
/// | Range / code point | Name |
/// |---|---|
/// | `U+0009`–`U+000D` | TAB, LF, VT, FF, CR |
/// | `U+0020` | SPACE |
/// | `U+00A0` | NO-BREAK SPACE |
/// | `U+1680` | OGHAM SPACE MARK |
/// | `U+2000`–`U+200A` | EN QUAD … HAIR SPACE |
/// | `U+2028`, `U+2029` | LINE / PARAGRAPH SEPARATOR |
/// | `U+202F` | NARROW NO-BREAK SPACE |
/// | `U+205F` | MEDIUM MATHEMATICAL SPACE |
/// | `U+3000` | IDEOGRAPHIC SPACE |
/// | `U+FEFF` | ZERO WIDTH NO-BREAK SPACE |
///
/// # Why not `char::is_whitespace`
///
/// The two sets are close but not equal, and both differences are reachable
/// from ordinary text:
///
/// | Character | here | `char::is_whitespace` |
/// |---|---|---|
/// | `U+0085` NEXT LINE | no | **yes** |
/// | `U+FEFF` ZERO WIDTH NBSP | **yes** | no |
///
/// `U+FEFF` is included because it is the byte-order mark, which arrives at the
/// head of the first line of a great many corpus files; a Brown line beginning
/// with one would otherwise produce a first token with an invisible character
/// glued to it, and that token becomes a feature of the trained model.
/// `U+0085` is excluded because it is a `Cc` control that no corpus in this
/// workspace contains, and because widening the set is not a free change here:
/// the tokens this function produces *are* the features of every maximum-entropy
/// model, and every saved model's stamp is computed over them. Changing the
/// class silently changes what an old model means rather than failing to load
/// it.
#[inline]
pub(crate) const fn is_whitespace(c: char) -> bool {
    matches!(c,
        '\u{0009}'..='\u{000D}'
        | '\u{0020}'
        | '\u{00A0}'
        | '\u{1680}'
        | '\u{2000}'..='\u{200A}'
        | '\u{2028}'
        | '\u{2029}'
        | '\u{202F}'
        | '\u{205F}'
        | '\u{3000}'
        | '\u{FEFF}'
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn differs_from_rust_whitespace_where_it_must() {
        // NEL: Rust says whitespace, this class does not.
        assert!('\u{0085}'.is_whitespace());
        assert!(!is_whitespace('\u{0085}'));

        // ZWNBSP: this class says whitespace, Rust does not.
        assert!(!'\u{FEFF}'.is_whitespace());
        assert!(is_whitespace('\u{FEFF}'));
    }

    #[test]
    fn covers_the_common_cases() {
        for c in [
            ' ', '\t', '\n', '\r', '\u{000B}', '\u{000C}', '\u{00A0}', '\u{3000}',
        ] {
            assert!(is_whitespace(c), "{c:?}");
        }
        for c in ['a', '0', '-', '\u{200B}' /* ZWSP is not in the set */] {
            assert!(!is_whitespace(c), "{c:?}");
        }
    }

    /// The whole table above, walked rather than sampled: every listed scalar
    /// is in the class and the scalars immediately outside each range are not.
    #[test]
    fn the_documented_set_is_exactly_this_one() {
        let listed: Vec<char> = ('\u{0009}'..='\u{000D}')
            .chain(['\u{0020}', '\u{00A0}', '\u{1680}'])
            .chain('\u{2000}'..='\u{200A}')
            .chain([
                '\u{2028}', '\u{2029}', '\u{202F}', '\u{205F}', '\u{3000}', '\u{FEFF}',
            ])
            .collect();
        for c in &listed {
            assert!(
                is_whitespace(*c),
                "{c:?} is documented but not in the class"
            );
        }
        // Every scalar up to U+FFFF that is *not* listed must be outside.
        for u in 0u32..=0xFFFF {
            let Some(c) = char::from_u32(u) else { continue };
            assert_eq!(
                is_whitespace(c),
                listed.contains(&c),
                "{c:?} (U+{u:04X}) disagrees with the documented table"
            );
        }
        // Nothing above the BMP is whitespace.
        for c in ['\u{10000}', '\u{1F600}', '\u{10FFFF}'] {
            assert!(!is_whitespace(c));
        }
    }
}
