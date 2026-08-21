//! A test-only global allocator that measures **live heap bytes on the calling
//! thread**, so a test can state a memory bound as an assertion.
//!
//! # Why this exists
//!
//! Two of this crate's memory bounds are not observable from the values the
//! code hands out. `for_each_deletion` promises `O(n)` live bytes at any
//! instant while emitting `Θ(n²)` variants, and `DeletionIndex` promises to
//! retain `O(n²)` bytes rather than the `Θ(n³)` a sequence-keyed map costs.
//! Both are claims about *peak live bytes*, and a test that watches what the
//! callback receives — the width of a slice, the number of emissions — cannot
//! see either: an implementation that materialises the whole set first and then
//! replays it satisfies every such assertion while allocating gigabytes. That
//! is not hypothetical; it is how the regression these bounds fix went
//! undetected. Counting the allocations is the one instrument that watches the
//! quantity being promised.
//!
//! # Per-thread, and why that is enough
//!
//! The counters are thread-local, so a test measures its own allocations and
//! not those of whatever else `cargo test` is running in parallel. Memory
//! allocated on one thread and freed on another therefore decrements the wrong
//! counter, which is why the count is signed; no measurement in this crate
//! spans threads, and every bound asserted here has orders of magnitude of
//! headroom over any stray byte.
//!
//! # `unsafe` policy: an exception, named as one
//!
//! The workspace sets `unsafe_code = "deny"`, and `GlobalAlloc` is an unsafe
//! trait, so this file overrides that lint. Three things about the override are
//! deliberate, and are stated here because this file ships:
//!
//! - **`expect`, not `allow`.** An `allow` would sit here forever after the
//!   unsafe it excuses had gone. `expect` is checked in the other direction
//!   too: the moment this file stops containing `unsafe`, the unfulfilled
//!   expectation is itself a lint, and `-D warnings` turns it into a build
//!   failure. The exception cannot outlive its reason.
//! - **`cfg(test)`, so no dependent ever compiles it.** The module is declared
//!   under `#[cfg(test)]` in `lib.rs` and reached only from this crate's own
//!   test modules. The published `rlib` a consumer links contains no `unsafe`
//!   from this crate, and `cargo build`/`cargo doc` never parse this file.
//! - **In the tarball all the same, on purpose.** `cargo package --list -p
//!   verbora-spellcheck` includes `src/counting_alloc.rs`, so anyone grepping
//!   the published source for `unsafe` finds it — which is why the reason is
//!   written here rather than in a commit message. It stays for two reasons.
//!   It cannot move to `tests/`: the property it measures belongs to
//!   `crate::deletions::for_each_deletion`, which is `pub(crate)` inside a
//!   private module, so an integration test cannot call it, and no public path
//!   exposes the transient peak (every public path *retains* an index, whose
//!   retention swamps the generator's transient bytes). And it cannot be
//!   excluded from the package: `lib.rs` declares the module, so a tarball
//!   without the file would not compile under `cargo test` — the one thing a
//!   packaged test suite exists for.
#![expect(
    unsafe_code,
    reason = "implementing `GlobalAlloc` is unsafe by definition; this instrument \
              exists only under `cfg(test)` and only forwards to `System`"
)]

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

thread_local! {
    /// Bytes currently allocated and not yet freed, on this thread.
    static LIVE: Cell<isize> = const { Cell::new(0) };
    /// The largest value `LIVE` has reached since the last [`measure`] began.
    static PEAK: Cell<isize> = const { Cell::new(0) };
}

/// The allocator itself: `System`, plus a thread-local tally.
pub(crate) struct Counting;

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        // SAFETY: the caller upholds `GlobalAlloc::alloc`'s contract, and this
        // forwards `layout` to `System` unchanged.
        let ptr = unsafe { System.alloc(layout) };
        if !ptr.is_null() {
            record(layout.size() as isize);
        }
        ptr
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        // SAFETY: as `alloc`, forwarding `layout` unchanged.
        let ptr = unsafe { System.alloc_zeroed(layout) };
        if !ptr.is_null() {
            record(layout.size() as isize);
        }
        ptr
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        // SAFETY: the caller guarantees `ptr` came from this allocator with
        // this `layout`, and this allocator only ever returns `System`'s
        // pointers for the layout it was given.
        unsafe { System.dealloc(ptr, layout) };
        record(-(layout.size() as isize));
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        // SAFETY: as `dealloc` for the old block, and `new_size` is passed
        // through unchanged for the new one.
        let new = unsafe { System.realloc(ptr, layout, new_size) };
        if !new.is_null() {
            record(new_size as isize - layout.size() as isize);
        }
        new
    }
}

/// Applies one allocation's signed size to this thread's counters.
///
/// `try_with` rather than `with`: an allocation can happen while thread-local
/// storage is being torn down, and a failed measurement must not abort the
/// process from inside the allocator.
fn record(delta: isize) {
    let _ = LIVE.try_with(|live| {
        let now = live.get() + delta;
        live.set(now);
        let _ = PEAK.try_with(|peak| {
            if now > peak.get() {
                peak.set(now);
            }
        });
    });
}

/// What [`measure`] observed, in bytes, relative to the moment it started.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Bytes {
    /// The most that was live at any one instant during the call.
    pub(crate) peak: usize,
    /// What was still live when the call returned — signed, because a body
    /// that frees more than it allocates is legitimate.
    pub(crate) retained: isize,
}

/// Runs `body`, reporting the heap it used and the heap it kept.
///
/// The value `body` produces is returned rather than dropped, so a caller
/// measuring what a structure *retains* can keep it alive: `retained` is read
/// before the value is handed back, and moving a value does not free its heap.
pub(crate) fn measure<T>(body: impl FnOnce() -> T) -> (Bytes, T) {
    let base = LIVE.with(Cell::get);
    PEAK.with(|peak| peak.set(base));
    let value = body();
    let end = LIVE.with(Cell::get);
    let peak = PEAK.with(Cell::get);
    let bytes = Bytes {
        peak: usize::try_from(peak - base).unwrap_or(0),
        retained: end - base,
    };
    (bytes, value)
}
