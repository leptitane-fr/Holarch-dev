//! The holon's memory: blocks of a power of two bytes (16 at least), each
//! size with its list of freed blocks to give again, new ones cut from
//! pages asked of the host (`memory.grow`). A block never changes size,
//! so the heap cannot crumble into pieces too small to use, whatever the
//! pages the browser opens and closes.

use core::alloc::{GlobalAlloc, Layout};
use core::arch::wasm32;
use core::ptr::{addr_of_mut, null_mut};

const PAGE: usize = 65_536;
const CLASSES: usize = usize::BITS as usize;

struct Heap;

/// The heap's state: the first freed block of each size, and the room
/// not cut yet.
struct State {
    free: [usize; CLASSES],
    next: usize,
    end: usize,
}

static mut STATE: State = State { free: [0; CLASSES], next: 0, end: 0 };

#[global_allocator]
static HEAP: Heap = Heap;

/// The size class of `layout`: the power of two that holds it.
fn class(layout: Layout) -> usize {
    layout.size().max(layout.align()).max(16).next_power_of_two().trailing_zeros() as usize
}

fn state() -> &'static mut State {
    // SAFETY: the holon runs one thread, and only the allocator, one call
    // at a time, reaches the state.
    unsafe { &mut *addr_of_mut!(STATE) }
}

unsafe impl GlobalAlloc for Heap {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let c = class(layout);
        let s = state();
        if s.free[c] != 0 {
            let block = s.free[c];
            // SAFETY: a freed block holds the address of the next one.
            s.free[c] = unsafe { *(block as *const usize) };
            return block as *mut u8;
        }
        if s.end == 0 {
            s.end = wasm32::memory_size(0) * PAGE;
            s.next = s.end;
        }
        let size = 1usize << c;
        let start = s.next.next_multiple_of(layout.align().max(16));
        let Some(stop) = start.checked_add(size) else { return null_mut() };
        if stop > s.end {
            let pages = (stop - s.end).div_ceil(PAGE);
            if wasm32::memory_grow(0, pages) == usize::MAX {
                return null_mut();
            }
            s.end += pages * PAGE;
        }
        s.next = stop;
        start as *mut u8
    }

    unsafe fn dealloc(&self, block: *mut u8, layout: Layout) {
        let c = class(layout);
        let s = state();
        // SAFETY: the block is ours again, at least 16 bytes: it keeps the
        // address of the next freed block of its size.
        unsafe { *(block as *mut usize) = s.free[c] };
        s.free[c] = block as usize;
    }

    unsafe fn realloc(&self, block: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        // SAFETY: the caller gives a valid layout and size (GlobalAlloc's
        // contract), kept for the new layout.
        let new = unsafe { Layout::from_size_align_unchecked(new_size, layout.align()) };
        if class(new) == class(layout) {
            return block;
        }
        // SAFETY: as above; the old block is valid for its size.
        unsafe {
            let fresh = self.alloc(new);
            if !fresh.is_null() {
                core::ptr::copy_nonoverlapping(block, fresh, layout.size().min(new_size));
                self.dealloc(block, layout);
            }
            fresh
        }
    }
}
