//! This crate exists to host Criterion benches under `benches/` and one
//! accuracy-report example under `examples/`, each pitting a real Verbora
//! crate against a real, pinned third-party competitor on identical input.
//! See `../../README.md`.
//!
//! The one piece of genuinely shared code — the §1.9-1.11 language-detection
//! dataset loader and cross-crate language-code mapping — lives in
//! [`language_support`], used by `benches/language.rs`,
//! `examples/language_accuracy.rs`, and
//! `tests/language_detection_correctness.rs` alike, so the three ISO-code
//! mapping tables are written exactly once.
//!
//! [`memory`] is the other shared piece: allocation-counting and RSS
//! measurement, installed below as the process's global allocator so it sees
//! every allocation any bench file or example makes, on both sides of every
//! comparison equally.
//!
//! [`double_metaphone_cpp`] is the one non-Rust competitor in this
//! workspace: an FFI binding to a vendored C++11 library, compiled by
//! `build.rs`. See that module's own doc comment.
//!
//! Note what is deliberately *absent* here: nothing in this library may name
//! the `eddie` crate. It is unsound (see
//! `tests/distance_correctness.rs`'s `eddie_slice` module) and is kept a
//! **dev-dependency** precisely so no shared helper can reach it — only the
//! one test module that wraps its sound slice-level API can.

pub mod double_metaphone_cpp;
pub mod language_support;
pub mod memory;

/// Installed process-wide so [`memory::measure`] sees every allocation any
/// benchmark or example in this crate makes — Verbora's and every
/// competitor's alike. See `memory`'s own doc comment for the fairness note
/// this implies.
#[global_allocator]
static GLOBAL_ALLOC: memory::CountingAllocator = memory::CountingAllocator;
