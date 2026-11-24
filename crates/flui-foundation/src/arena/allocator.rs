//! How a [`FrameArena`] hands out memory: the chunks it takes from the global allocator, the
//! cursor it bumps through them, and the [`Allocator`] implementation collections allocate
//! through.
//!
//! Nothing here panics. A collection trusts its allocator not to unwind: std's collections are
//! unsound when an `Allocator` method or its `drop` unwinds, and the trait as stabilised in Rust
//! 1.100 forbids it. Every failure is therefore an [`AllocError`]: a chunk the global allocator
//! refuses, a size past `usize`, a list of chunks that cannot grow or is already borrowed. The
//! lints below refuse the panicking forms a later edit could bring in. Two forms no lint names
//! stay out by hand: the list of chunks is borrowed with `try_borrow_mut`, never `borrow_mut`,
//! and grows only after `try_reserve` made room for the push.
#![deny(
    clippy::arithmetic_side_effects,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::todo,
    clippy::unimplemented,
    clippy::unreachable,
    clippy::unwrap_used
)]

use std::alloc::{AllocError, Allocator, Global, Layout};
use std::ptr::NonNull;

use super::{FrameArena, MIN_CHUNK};

/// Every chunk's alignment; a block aligned more strictly is padded inside its chunk.
const CHUNK_ALIGN: usize = 16;

/// One block of the global allocator's memory the arena bumps through.
pub(super) struct Chunk {
    pub(super) ptr: NonNull<u8>,
    layout: Layout,
}

impl Chunk {
    /// `size` must be above zero.
    fn try_new(size: usize) -> Result<Self, AllocError> {
        let layout = Layout::from_size_align(size, CHUNK_ALIGN).map_err(|_| AllocError)?;
        let ptr = Global.allocate(layout)?.cast::<u8>();
        Ok(Self { ptr, layout })
    }

    pub(super) fn capacity(&self) -> usize {
        self.layout.size()
    }
}

impl Drop for Chunk {
    fn drop(&mut self) {
        // SAFETY: `Chunk::try_new` took `ptr` from `Global` with exactly `layout`, and a chunk is
        // freed only here, once.
        unsafe { Global.deallocate(self.ptr, self.layout) }
    }
}

impl FrameArena {
    /// Bytes handed out since the last reset, padding included.
    pub(super) fn used(&self) -> usize {
        self.retired.get().saturating_add(self.cursor.get())
    }

    /// The offset of `ptr` in the current chunk when the block `[ptr, ptr + size)` is the last
    /// one handed out from it.
    fn last_block_offset(&self, ptr: NonNull<u8>, size: usize) -> Option<usize> {
        let offset = ptr.addr().get().checked_sub(self.base.get().addr().get())?;
        (offset.checked_add(size)? == self.cursor.get()).then_some(offset)
    }

    /// Hands out `layout` from the current chunk, or `None` when it does not fit there.
    fn bump(&self, layout: Layout) -> Option<NonNull<[u8]>> {
        let base = self.base.get().addr().get();
        let start = base
            .checked_add(self.cursor.get())?
            .checked_next_multiple_of(layout.align())?;
        let offset = start.checked_sub(base)?;
        let end = offset.checked_add(layout.size())?;
        if end > self.capacity.get() {
            return None;
        }
        self.cursor.set(end);
        self.peak.set(self.peak.get().max(self.used()));
        // SAFETY: `offset + layout.size() <= capacity`, so the block lies inside the current
        // chunk's allocation (`base` is dangling only while `capacity` is zero, and `end` above
        // zero exceeds it).
        let ptr = unsafe { self.base.get().add(offset) };
        Some(NonNull::slice_from_raw_parts(ptr, layout.size()))
    }

    /// Opens a chunk that holds `layout`, at least twice the current one.
    fn open_chunk(&self, layout: Layout) -> Result<(), AllocError> {
        let size = layout
            .size()
            .checked_add(layout.align())
            .ok_or(AllocError)?
            .max(self.capacity.get().saturating_mul(2))
            .max(self.next_chunk)
            .max(MIN_CHUNK)
            .checked_next_power_of_two()
            .ok_or(AllocError)?;
        let mut chunks = self.chunks.try_borrow_mut().map_err(|_| AllocError)?;
        chunks.try_reserve(1).map_err(|_| AllocError)?;
        let chunk = Chunk::try_new(size)?;
        let ptr = chunk.ptr;
        // Within the capacity `try_reserve` made, so the list does not grow here.
        chunks.push(chunk);
        drop(chunks);
        self.retired.set(self.used());
        self.base.set(ptr);
        self.capacity.set(size);
        self.cursor.set(0);
        Ok(())
    }
}

/// A zero-size block: a dangling pointer aligned for `layout`.
fn empty_block(layout: Layout) -> NonNull<[u8]> {
    NonNull::slice_from_raw_parts(layout.dangling_ptr(), 0)
}

// SAFETY: a block is handed out at most once between two resets: `bump` moves the cursor past
// every block it returns, and the cursor moves back only over the last block, when that block is
// freed or shrunk. A block stays valid while the `&'f FrameArena` that handed it out lives: its
// chunk is freed only by `reset`, which needs `&mut FrameArena`, or by the arena's drop, and
// neither can happen while `'f` lasts. Copies of the reference are the same allocator, so any
// copy may free or grow a block another copy handed out.
unsafe impl Allocator for &FrameArena {
    fn allocate(&self, layout: Layout) -> Result<NonNull<[u8]>, AllocError> {
        if layout.size() == 0 {
            return Ok(empty_block(layout));
        }
        if let Some(block) = self.bump(layout) {
            return Ok(block);
        }
        self.open_chunk(layout)?;
        self.bump(layout).ok_or(AllocError)
    }

    unsafe fn deallocate(&self, ptr: NonNull<u8>, layout: Layout) {
        if layout.size() == 0 {
            return;
        }
        if let Some(offset) = self.last_block_offset(ptr, layout.size()) {
            self.cursor.set(offset);
        }
    }

    unsafe fn grow(
        &self,
        ptr: NonNull<u8>,
        old_layout: Layout,
        new_layout: Layout,
    ) -> Result<NonNull<[u8]>, AllocError> {
        if old_layout.size() > 0
            && ptr.addr().get().is_multiple_of(new_layout.align())
            && let Some(offset) = self.last_block_offset(ptr, old_layout.size())
            && let Some(end) = offset.checked_add(new_layout.size())
            && end <= self.capacity.get()
        {
            self.cursor.set(end);
            self.peak.set(self.peak.get().max(self.used()));
            return Ok(NonNull::slice_from_raw_parts(ptr, new_layout.size()));
        }
        let new = self.allocate(new_layout)?;
        // SAFETY: the caller guarantees `ptr` holds `old_layout.size()` initialised bytes and
        // `new_layout.size() >= old_layout.size()`; `new` is a different block, since the old
        // one is still handed out, so the two do not overlap.
        unsafe {
            std::ptr::copy_nonoverlapping(
                ptr.as_ptr(),
                new.cast::<u8>().as_ptr(),
                old_layout.size(),
            );
            self.deallocate(ptr, old_layout);
        }
        Ok(new)
    }

    unsafe fn shrink(
        &self,
        ptr: NonNull<u8>,
        old_layout: Layout,
        new_layout: Layout,
    ) -> Result<NonNull<[u8]>, AllocError> {
        if new_layout.size() == 0 {
            // SAFETY: the caller hands over the block `ptr` with `old_layout`.
            unsafe { self.deallocate(ptr, old_layout) };
            return Ok(empty_block(new_layout));
        }
        if ptr.addr().get().is_multiple_of(new_layout.align()) {
            if let Some(offset) = self.last_block_offset(ptr, old_layout.size())
                && let Some(end) = offset.checked_add(new_layout.size())
            {
                self.cursor.set(end);
            }
            return Ok(NonNull::slice_from_raw_parts(ptr, new_layout.size()));
        }
        let new = self.allocate(new_layout)?;
        // SAFETY: the caller guarantees `ptr` holds at least `new_layout.size()` initialised
        // bytes; `new` is a different block, since the old one is still handed out.
        unsafe {
            std::ptr::copy_nonoverlapping(
                ptr.as_ptr(),
                new.cast::<u8>().as_ptr(),
                new_layout.size(),
            );
            self.deallocate(ptr, old_layout);
        }
        Ok(new)
    }
}
