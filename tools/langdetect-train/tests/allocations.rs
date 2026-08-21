//! Allocation-count assertions for `HashedLinearDetector::detect`.
//!
//! Lives here rather than in `verbora-language` because a counting global
//! allocator requires `unsafe impl GlobalAlloc`, and this tool sits outside
//! the workspace (see Cargo.toml), so it inherits no `unsafe_code` lint at
//! all. A shipped crate *can* carry one where the property is not otherwise
//! observable — `verbora-spellcheck`'s test-only `counting_alloc` does, with
//! a recorded `expect` — but nothing here needs that exemption. The
//! contract under test is the report's 0-alloc inference claim: the
//! scoring loop allocates nothing; the only allocation on any path is the
//! single-candidate `Vec` the public `LanguageDetection` shape requires,
//! and an abstaining call performs no allocation at all.

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicU64, Ordering};

use verbora_language::{HashedLinearDetector, LanguageDetector};

struct Counting;

static ALLOCS: AtomicU64 = AtomicU64::new(0);

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        ALLOCS.fetch_add(1, Ordering::Relaxed);
        unsafe { System.alloc(layout) }
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) }
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        ALLOCS.fetch_add(1, Ordering::Relaxed);
        unsafe { System.realloc(ptr, layout, new_size) }
    }
}

#[global_allocator]
static COUNTER: Counting = Counting;

/// Allocations across 10 identical calls, divided by 10 — warm-up call
/// first so any one-time lazy state (there should be none) is excluded.
fn allocs_per_call(detector: &HashedLinearDetector, input: &str) -> u64 {
    let _ = detector.detect(input);
    let before = ALLOCS.load(Ordering::Relaxed);
    for _ in 0..10 {
        std::hint::black_box(detector.detect(std::hint::black_box(input)));
    }
    (ALLOCS.load(Ordering::Relaxed) - before) / 10
}

#[test]
fn abstaining_detect_allocates_nothing() {
    let d = HashedLinearDetector::new();
    for input in [
        "",
        "12345 !!!",
        "😀😀😀",
        "العربية نص",        // Arabic abstention guard
        "한국어 문장입니다", // unsupported script
    ] {
        assert_eq!(
            allocs_per_call(&d, input),
            0,
            "abstaining detect({input:?}) must not allocate"
        );
    }
}

#[test]
fn successful_detect_allocates_exactly_the_candidate_vec() {
    let d = HashedLinearDetector::new();
    for input in [
        "The weather is beautiful today and the children are playing outside.",
        "Сегодня прекрасная погода, и дети играют на улице в парке.",
        "これはにほんごのぶんしょうです",
        "中文文章看起来是这样的",
        "आज मौसम बहुत सुहावना है",
    ] {
        assert_eq!(
            allocs_per_call(&d, input),
            1,
            "detect({input:?}) must allocate exactly once (the candidates Vec)"
        );
    }
}
