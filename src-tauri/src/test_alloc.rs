//! The test binary's counting allocator: wraps the system one and counts
//! allocations, bytes allocated and live bytes, for the allocation
//! benchmarks in the desktop (`gpui_shell`) and mobile (`mobile`) tests.
//! A binary can register only one global allocator, so both share this one.

use std::{
  alloc::{GlobalAlloc, Layout, System},
  sync::atomic::{AtomicIsize, AtomicUsize, Ordering::Relaxed},
};

struct Counting;

pub(crate) static ALLOCS: AtomicUsize = AtomicUsize::new(0);
pub(crate) static BYTES: AtomicUsize = AtomicUsize::new(0);
pub(crate) static LIVE: AtomicIsize = AtomicIsize::new(0);

// SAFETY: forwards every call to `System` unchanged, only counting.
unsafe impl GlobalAlloc for Counting {
  unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
    ALLOCS.fetch_add(1, Relaxed);
    BYTES.fetch_add(layout.size(), Relaxed);
    LIVE.fetch_add(layout.size() as isize, Relaxed);
    // SAFETY: the caller upholds `alloc`'s contract.
    unsafe { System.alloc(layout) }
  }

  unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
    LIVE.fetch_sub(layout.size() as isize, Relaxed);
    // SAFETY: the caller upholds `dealloc`'s contract.
    unsafe { System.dealloc(ptr, layout) }
  }

  unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
    ALLOCS.fetch_add(1, Relaxed);
    BYTES.fetch_add(new_size, Relaxed);
    LIVE.fetch_add(new_size as isize - layout.size() as isize, Relaxed);
    // SAFETY: the caller upholds `realloc`'s contract.
    unsafe { System.realloc(ptr, layout, new_size) }
  }
}

#[global_allocator]
static COUNTING: Counting = Counting;
