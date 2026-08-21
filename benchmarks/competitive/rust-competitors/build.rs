//! Compiles the vendored `pixelglow/double_metaphone` C++11 header (via a
//! thin C-ABI shim in the same directory) so
//! `tests/double_metaphone_cpp_correctness.rs` can check Verbora against
//! it through FFI. See
//! `vendor/pixelglow-double_metaphone/README.md` for the library's
//! provenance and license, and `src/double_metaphone_cpp.rs` for the Rust
//! side of the binding.
//!
//! This is the only compiled-from-source, non-Rust competitor in this
//! workspace's dependency tree -- every other competitor is a normal Cargo
//! crate. A C++ toolchain (`cc`/`c++`, any C++11-capable compiler) is
//! required to build this crate as a result; see `README.md`.

fn main() {
    cc::Build::new()
        .cpp(true)
        .std("c++11")
        .include("vendor/pixelglow-double_metaphone")
        .file("vendor/pixelglow-double_metaphone/shim.cpp")
        .warnings(true)
        .compile("pixelglow_double_metaphone");

    println!("cargo:rerun-if-changed=vendor/pixelglow-double_metaphone/shim.cpp");
    println!("cargo:rerun-if-changed=vendor/pixelglow-double_metaphone/double_metaphone.h");
}
