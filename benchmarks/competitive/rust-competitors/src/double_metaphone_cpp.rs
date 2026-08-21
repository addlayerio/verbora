//! FFI binding to the vendored `pixelglow/double_metaphone` C++11 header
//! (compiled and linked by `build.rs`). See
//! `vendor/pixelglow-double_metaphone/README.md` for provenance and
//! license, and `tests/double_metaphone_cpp_correctness.rs` for the
//! comparison against `verbora_phonetics::DoubleMetaphone` — including why
//! that comparison is correctness-only, with no companion benchmark.

use std::ffi::{CStr, CString};
use std::os::raw::c_char;

unsafe extern "C" {
    fn dm_double_metaphone(
        input: *const c_char,
        primary_out: *mut c_char,
        primary_cap: usize,
        secondary_out: *mut c_char,
        secondary_cap: usize,
    );
}

/// Generous enough for any realistic benchmark input -- double metaphone
/// keys stay a small multiple of the input length, and every word/name in
/// this workspace's shared datasets is well under 20 characters. See
/// `shim.cpp`'s own doc comment for the truncation behavior if this were
/// ever exceeded.
const BUF_CAP: usize = 256;

/// Safe wrapper around the vendored C++ `dm::double_metaphone`: returns
/// (primary key, secondary key), directly comparable to
/// [`verbora_phonetics::DoubleMetaphone::process`]'s return shape.
///
/// # Panics
/// If `word` contains an interior NUL byte (never true for real text) or if
/// the C++ side ever produced non-UTF-8 output (never true: the algorithm's
/// output alphabet is a fixed set of ASCII letters).
pub fn double_metaphone_cpp(word: &str) -> (String, String) {
    let input = CString::new(word).expect("benchmark input never contains an interior NUL");
    let mut primary = [0 as c_char; BUF_CAP];
    let mut secondary = [0 as c_char; BUF_CAP];

    // SAFETY: `primary`/`secondary` are valid, writable buffers of exactly
    // `BUF_CAP` bytes each, and `dm_double_metaphone` (see shim.cpp) never
    // writes more than `cap` bytes including the NUL terminator into either.
    unsafe {
        dm_double_metaphone(
            input.as_ptr(),
            primary.as_mut_ptr(),
            BUF_CAP,
            secondary.as_mut_ptr(),
            BUF_CAP,
        );
    }

    // SAFETY: the shim always NUL-terminates both buffers before returning.
    let primary = unsafe { CStr::from_ptr(primary.as_ptr()) };
    let secondary = unsafe { CStr::from_ptr(secondary.as_ptr()) };

    (
        primary
            .to_str()
            .expect("double_metaphone's output alphabet is ASCII")
            .to_owned(),
        secondary
            .to_str()
            .expect("double_metaphone's output alphabet is ASCII")
            .to_owned(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The vendored library's own test.cpp pins this exact case -- a smoke
    /// test that the FFI plumbing itself works before any correctness test
    /// runs the full shared word list.
    #[test]
    fn matches_upstreams_own_worked_example() {
        assert_eq!(
            double_metaphone_cpp("Angier"),
            ("ANJ".into(), "ANJR".into())
        );
    }
}
