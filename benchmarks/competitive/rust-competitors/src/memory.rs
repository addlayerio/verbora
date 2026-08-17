//! Allocation counting and peak-RSS measurement, for the memory dimension
//! `Fase 6 Benchmark.md`'s own `MEMORY BENCHMARKS` section asks for
//! ("Performance NO significa solamente tiempo") and the independent
//! fairness audit's final validation pass flagged as entirely absent from
//! this suite (Definition of Done item 21: `PARTIAL`).
//!
//! # Why this lives here, not in the main workspace
//!
//! [`CountingAllocator`] implements the `unsafe` [`GlobalAlloc`] trait. The
//! root workspace denies `unsafe_code` outright (`AGENTS.md`'s policy);
//! `benchmarks/competitive`'s own, separate `[workspace]` does not inherit
//! that lint, precisely because this crate never ships as a dependency of
//! anything — it exists only to measure. This mirrors the same workaround
//! `crates/verbora-language/benches/language.rs`'s own allocation-counting
//! probe used (a separate, non-workspace Cargo project) for exactly the same
//! reason, just simpler here since this crate is *already* outside the
//! restricted workspace.
//!
//! # What this measures, and what it does not
//!
//! [`measure`] wraps a closure and reports how many times the *global*
//! allocator was asked to allocate/deallocate/reallocate, and how many bytes,
//! during that closure's execution — real counts, not an estimate from
//! reading source. It is installed as the process's `#[global_allocator]`
//! (see `lib.rs`), so it sees every allocation any code path makes, Verbora's
//! and every competitor's alike, on equal footing.
//!
//! It does **not** measure peak heap *size* (the high-water mark of bytes
//! simultaneously live) — only a running total of bytes requested and freed.
//! [`rss_kb`] is the complementary, coarser measurement for that: it reads
//! the whole process's resident set size from `/proc/self/status`, which
//! captures peak-ish real memory pressure (page cache, thread stacks, and
//! every other live allocation in the process included) rather than one
//! call's isolated cost — the two are reported side by side, never
//! conflated, per this file's own [`Report`] shape.
//!
//! # Fairness note
//!
//! Every benchmark in this crate runs under this same global allocator,
//! whether or not it is being measured for memory this run — a few atomic
//! increments per allocation call, paid identically by Verbora and every
//! competitor. This is a real, small, *shared* tax, not a bias toward either
//! side of any comparison; documented here rather than left implicit.
//!
//! # Single-threaded measurement only
//!
//! The counters (`ALLOC_COUNT` and friends) are process-global, not
//! per-thread — [`measure`] reports whatever the *whole process* allocated
//! between its two snapshots, including anything any other concurrently
//! running thread did in that window. This is exactly right for its real
//! call sites (a dedicated `examples/memory_report.rs`-style binary run
//! alone, or one Criterion iteration with nothing else scheduled) and wrong
//! for `cargo test`'s default multi-threaded runner, where sibling tests
//! allocate on other threads at unpredictable moments — this module's own
//! tests were written to be robust to that (relative comparisons, not exact
//! zero-allocation assertions) for exactly this reason; see
//! `measure_distinguishes_an_allocating_closure_from_a_non_allocating_one`.

use std::alloc::{GlobalAlloc, Layout, System};
use std::fs;
use std::sync::atomic::{AtomicU64, Ordering};

/// Wraps [`System`], counting calls and bytes without changing behavior.
///
/// # Safety
///
/// Every method is a direct pass-through to `System`'s own implementation of
/// the same method, with only non-allocating counter updates added around
/// it — it upholds [`GlobalAlloc`]'s safety contract exactly because it does
/// not touch the contract-bearing part of the call at all.
pub struct CountingAllocator;

static ALLOC_COUNT: AtomicU64 = AtomicU64::new(0);
static ALLOC_BYTES: AtomicU64 = AtomicU64::new(0);
static DEALLOC_COUNT: AtomicU64 = AtomicU64::new(0);
static DEALLOC_BYTES: AtomicU64 = AtomicU64::new(0);

// SAFETY: every method below only ever forwards to `System`'s own
// implementation of the identical method with the identical arguments; nоthing
// here changes what memory is returned, reused, or freed. The counters are
// plain atomics, updated after (alloc) or before (dealloc) the real call,
// never read by the allocator itself, so there is no reentrancy hazard from
// the bookkeeping.
unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        ALLOC_COUNT.fetch_add(1, Ordering::Relaxed);
        ALLOC_BYTES.fetch_add(layout.size() as u64, Ordering::Relaxed);
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        DEALLOC_COUNT.fetch_add(1, Ordering::Relaxed);
        DEALLOC_BYTES.fetch_add(layout.size() as u64, Ordering::Relaxed);
        unsafe { System.dealloc(ptr, layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        // A realloc is one call, not an alloc-then-dealloc pair -- counted
        // as its own alloc event sized at the delta, matching how a caller
        // would read "how many times did the allocator do work, and how many
        // net bytes did that work move."
        ALLOC_COUNT.fetch_add(1, Ordering::Relaxed);
        if new_size >= layout.size() {
            ALLOC_BYTES.fetch_add((new_size - layout.size()) as u64, Ordering::Relaxed);
        } else {
            DEALLOC_BYTES.fetch_add((layout.size() - new_size) as u64, Ordering::Relaxed);
        }
        unsafe { System.realloc(ptr, layout, new_size) }
    }
}

/// A raw counter snapshot, taken with [`AllocStats::snapshot`].
#[derive(Debug, Clone, Copy)]
struct AllocStats {
    alloc_count: u64,
    alloc_bytes: u64,
    dealloc_count: u64,
    dealloc_bytes: u64,
}

impl AllocStats {
    fn snapshot() -> Self {
        Self {
            alloc_count: ALLOC_COUNT.load(Ordering::Relaxed),
            alloc_bytes: ALLOC_BYTES.load(Ordering::Relaxed),
            dealloc_count: DEALLOC_COUNT.load(Ordering::Relaxed),
            dealloc_bytes: DEALLOC_BYTES.load(Ordering::Relaxed),
        }
    }
}

/// What one [`measure`] call reports.
#[derive(Debug, Clone, Copy, serde::Serialize)]
pub struct Report {
    /// Number of `alloc`/`realloc` calls the closure caused.
    pub allocations: u64,
    /// Total bytes requested across those calls (a `realloc` counts only its
    /// growth, never the whole new size, to avoid double-counting bytes the
    /// allocation already held).
    pub bytes_allocated: u64,
    /// Number of `dealloc`/shrinking-`realloc` calls the closure caused.
    pub deallocations: u64,
    /// Total bytes freed across those calls.
    pub bytes_deallocated: u64,
    /// Process resident-set size immediately after the closure returns, in
    /// KiB, or `None` off Linux / if `/proc` is unreadable. A coarse,
    /// whole-process figure -- see this module's own doc comment for why it
    /// is reported separately from the precise allocation counts above.
    pub rss_kb_after: Option<u64>,
}

/// Runs `f`, reporting how many allocator calls it caused and the process's
/// RSS immediately afterward.
///
/// Call this only around a single, already-warmed-up operation -- there is
/// no batching/statistics here (allocation counts do not have Criterion's
/// noise problem: the same code makes the same calls every time, so one
/// clean measurement is the real number, not an estimate).
pub fn measure<T>(f: impl FnOnce() -> T) -> (T, Report) {
    let before = AllocStats::snapshot();
    let result = f();
    let after = AllocStats::snapshot();
    let report = Report {
        allocations: after.alloc_count - before.alloc_count,
        bytes_allocated: after.alloc_bytes - before.alloc_bytes,
        deallocations: after.dealloc_count - before.dealloc_count,
        bytes_deallocated: after.dealloc_bytes - before.dealloc_bytes,
        rss_kb_after: rss_kb(),
    };
    (result, report)
}

/// The process's current resident-set size, in KiB, read from
/// `/proc/self/status`'s `VmRSS:` line. `None` if unreadable (non-Linux, or
/// a sandboxed environment without `/proc`).
#[must_use]
pub fn rss_kb() -> Option<u64> {
    let status = fs::read_to_string("/proc/self/status").ok()?;
    status.lines().find_map(|line| {
        let rest = line.strip_prefix("VmRSS:")?;
        rest.trim().strip_suffix(" kB")?.trim().parse().ok()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn measure_counts_a_known_allocation() {
        let (_, report) = measure(|| {
            let v: Vec<u8> = Vec::with_capacity(4096);
            std::hint::black_box(v)
        });
        assert!(
            report.allocations >= 1,
            "Vec::with_capacity(4096) must allocate at least once"
        );
        assert!(report.bytes_allocated >= 4096);
    }

    /// Not an exact-zero assertion: the counters are process-global (see this
    /// module's own "Single-threaded measurement only" doc section), so a
    /// sibling test allocating on another thread during this closure's
    /// window would make an exact-zero check flaky under `cargo test`'s
    /// default parallel runner — confirmed by reproducing exactly this
    /// flake while writing this test. A relative comparison against a
    /// closure that *must* allocate is robust to that noise in the
    /// direction that matters: cross-thread contamination can only ever
    /// inflate both sides' counts, never invert the inequality.
    #[test]
    fn measure_distinguishes_an_allocating_closure_from_a_non_allocating_one() {
        let (_, arithmetic) = measure(|| std::hint::black_box(2 + 2));
        let (_, allocating) = measure(|| {
            let v: Vec<u8> = Vec::with_capacity(1_000_000);
            std::hint::black_box(v)
        });
        assert!(
            allocating.bytes_allocated > arithmetic.bytes_allocated + 900_000,
            "a 1MB allocation must be clearly distinguishable from background \
             noise: arithmetic={arithmetic:?} allocating={allocating:?}"
        );
    }

    #[test]
    fn rss_kb_reports_something_plausible_on_linux() {
        if let Some(kb) = rss_kb() {
            // A running test binary is at minimum a few hundred KiB, and
            // nowhere near the terabyte range -- a loose sanity bound, not a
            // precise assertion, since exact RSS is inherently unstable.
            assert!(kb > 100, "implausibly small RSS: {kb} KiB");
            assert!(kb < 100_000_000, "implausibly large RSS: {kb} KiB");
        }
    }
}
