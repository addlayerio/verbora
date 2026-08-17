//! The reference runtime `Buffer` encodings `addFileSync` decodes with.
//!
//! `addFileSync` hands its `encoding` argument to `fs.readFileSync`, which
//! returns a **string**, so the encoding is not merely a validation step: it
//! decides what text gets tokenized. Reading a UTF-8 file as `base64` yields the
//! *base64 text of the bytes*, which is then lowercased and split — and the reference
//! io_spec asserts the resulting token
//! `dghpcybkb2n1bwvudcbpcybhym91dcbub2rllg`.
//!
//! # The dead switch
//!
//! `tfidf` carries an `isEncoding` helper with a hand-written switch "for
//! < node 0.10". On any modern Node the first line wins and the switch is dead
//! code — which matters, because the switch accepts `'raw'` and
//! `Buffer.isEncoding` does not. The accepted set below is the live one,
//! confirmed against `Buffer.isEncoding` rather than read off the switch.

/// A Node `Buffer` encoding.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Encoding {
    /// `utf8`, `utf-8`.
    Utf8,
    /// `ascii` — each byte masked to seven bits.
    Ascii,
    /// `latin1`, `binary` — each byte is one code point.
    Latin1,
    /// `base64`.
    Base64,
    /// `base64url`.
    Base64Url,
    /// `hex`.
    Hex,
    /// `ucs2`, `ucs-2`, `utf16le`, `utf-16le`.
    Utf16Le,
}

impl Encoding {
    /// Resolves a Node encoding name, case-insensitively.
    ///
    /// Returns `None` exactly when `Buffer.isEncoding` returns `false`, which
    /// is what makes `addFileSync` throw `Invalid encoding: <name>`.
    pub fn parse(name: &str) -> Option<Self> {
        // `Buffer.isEncoding` lowercases with ASCII semantics before matching.
        let lower = name.to_ascii_lowercase();
        Some(match lower.as_str() {
            "utf8" | "utf-8" => Self::Utf8,
            "ascii" => Self::Ascii,
            "latin1" | "binary" => Self::Latin1,
            "base64" => Self::Base64,
            "base64url" => Self::Base64Url,
            "hex" => Self::Hex,
            "ucs2" | "ucs-2" | "utf16le" | "utf-16le" => Self::Utf16Le,
            _ => return None,
        })
    }

    /// Decodes `bytes` the way `Buffer#toString(encoding)` does.
    pub fn decode(self, bytes: &[u8]) -> String {
        match self {
            // Node substitutes U+FFFD for malformed sequences, as does
            // `from_utf8_lossy`.
            Self::Utf8 => String::from_utf8_lossy(bytes).into_owned(),
            Self::Ascii => bytes.iter().map(|b| char::from(b & 0x7f)).collect(),
            Self::Latin1 => bytes.iter().map(|b| char::from(*b)).collect(),
            Self::Base64 => base64(bytes, BASE64_STANDARD, true),
            Self::Base64Url => base64(bytes, BASE64_URL, false),
            Self::Hex => {
                let mut out = String::with_capacity(bytes.len() * 2);
                for b in bytes {
                    out.push(HEX[usize::from(b >> 4)]);
                    out.push(HEX[usize::from(b & 0xf)]);
                }
                out
            }
            Self::Utf16Le => {
                // A trailing odd byte is dropped, as Node does.
                let units: Vec<u16> = bytes
                    .chunks_exact(2)
                    .map(|c| u16::from_le_bytes([c[0], c[1]]))
                    .collect();
                String::from_utf16_lossy(&units)
            }
        }
    }
}

const HEX: [char; 16] = [
    '0', '1', '2', '3', '4', '5', '6', '7', '8', '9', 'a', 'b', 'c', 'd', 'e', 'f',
];

const BASE64_STANDARD: &[u8; 64] =
    b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
const BASE64_URL: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";

/// Encodes bytes as base64 text. `base64url` output is unpadded, as in Node.
fn base64(bytes: &[u8], alphabet: &[u8; 64], pad: bool) -> String {
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    let mut chunks = bytes.chunks_exact(3);
    for c in &mut chunks {
        let n = (u32::from(c[0]) << 16) | (u32::from(c[1]) << 8) | u32::from(c[2]);
        for shift in [18, 12, 6, 0] {
            out.push(char::from(alphabet[((n >> shift) & 0x3f) as usize]));
        }
    }
    match chunks.remainder() {
        [a] => {
            let n = u32::from(*a) << 16;
            out.push(char::from(alphabet[((n >> 18) & 0x3f) as usize]));
            out.push(char::from(alphabet[((n >> 12) & 0x3f) as usize]));
            if pad {
                out.push_str("==");
            }
        }
        [a, b] => {
            let n = (u32::from(*a) << 16) | (u32::from(*b) << 8);
            for shift in [18, 12, 6] {
                out.push(char::from(alphabet[((n >> shift) & 0x3f) as usize]));
            }
            if pad {
                out.push('=');
            }
        }
        _ => {}
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_exactly_what_buffer_is_encoding_accepts() {
        for name in [
            "utf8",
            "UTF8",
            "utf-8",
            "ascii",
            "ASCII",
            "base64",
            "BASE64",
            "base64url",
            "hex",
            "HEX",
            "ucs2",
            "ucs-2",
            "utf16le",
            "utf-16le",
            "UTF-16LE",
            "binary",
            "BINARY",
            "latin1",
            "Latin1",
        ] {
            assert!(Encoding::parse(name).is_some(), "{name} should be accepted");
        }
        // `raw` is in the module's dead legacy switch but not in Buffer.
        for name in ["raw", "", "foobar", "buffer", "utf9", "utf-8 "] {
            assert!(Encoding::parse(name).is_none(), "{name} should be rejected");
        }
    }

    #[test]
    fn base64_matches_node() {
        // Node: Buffer.from('this document is about node.').toString('base64')
        assert_eq!(
            Encoding::Base64.decode(b"this document is about node."),
            "dGhpcyBkb2N1bWVudCBpcyBhYm91dCBub2RlLg=="
        );
        assert_eq!(Encoding::Base64.decode(b"a"), "YQ==");
        assert_eq!(Encoding::Base64.decode(b"ab"), "YWI=");
        assert_eq!(Encoding::Base64.decode(b"abc"), "YWJj");
        assert_eq!(Encoding::Base64.decode(b""), "");
        assert_eq!(Encoding::Base64Url.decode(&[0xfb, 0xff]), "-_8");
    }

    #[test]
    fn hex_and_byte_encodings_match_node() {
        assert_eq!(Encoding::Hex.decode(b"one"), "6f6e65");
        assert_eq!(Encoding::Ascii.decode(&[0xe9, b'a']), "ia");
        assert_eq!(Encoding::Latin1.decode(&[0xe9, b'a']), "éa");
        // An odd trailing byte is dropped.
        assert_eq!(Encoding::Utf16Le.decode(&[0x61, 0x00, 0x62]), "a");
    }
}
