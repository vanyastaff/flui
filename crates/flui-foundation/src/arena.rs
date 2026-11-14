//! Scratch memory that one owner's frame walks share and give back all at once (ADR-0328).
//!
//! A walk that needs a list only while it runs takes the list from a [`FrameArena`] instead of
//! the global allocator: `Vec::new_in(&arena)`, written [`FrameVec`]. The list borrows the arena,
//! and [`FrameArena::reset`] takes `&mut self`, so no list is alive when its memory is reused; the
//! compiler checks it, and nothing runs on the frame path to check it again.
//!
//! The arena bumps a cursor through chunks it took from the global allocator. Freeing the last
//! block handed out moves the cursor back, and growing it extends it in place, so a walk that
//! frees in the reverse order it allocated (each level of a recursion, a list pushed to and then
//! consumed) reuses its memory within the frame. Any other block waits for `reset`. After a reset
//! the arena keeps one chunk large enough for the frames it served recently, so a steady frame
//! takes nothing from the global allocator, and a chunk one heavy frame grew is released once
//! 120 resets that used the arena have passed without needing it.
//!
//! ```
//! use flui_foundation::arena::{FrameArena, FrameVec};
//!
//! let mut arena = FrameArena::new();
//! for frame in 0..3_u32 {
//!     let mut squares: FrameVec<'_, u32> = Vec::new_in(&arena);
//!     squares.extend((0..frame).map(|n| n * n));
//!     assert_eq!(squares.len(), frame as usize);
//!     drop(squares);
//!     arena.reset();
//! }
//! ```

use std::alloc::{AllocError, Allocator, Global, Layout};
use std::cell::{Cell, RefCell};
use std::fmt;
use std::ptr::NonNull;

/// A `Vec` whose memory comes from a [`FrameArena`] and lives until the arena's next reset.
pub type FrameVec<'f, T> = Vec<T, &'f FrameArena>;

/// How many resets the arena remembers when it decides what to keep: a chunk more than
/// [`RELEASE_FACTOR`] times larger than any of these resets needed is released.
const RECENT_RESETS: usize = 120;

/// The smallest chunk the arena takes from the global allocator.
const MIN_CHUNK: usize = 4 * 1024;

/// Every chunk's alignment; a block aligned more strictly is padded inside its chunk.
const CHUNK_ALIGN: usize = 16;

/// How much larger than the recent need a kept chunk may be.
const RELEASE_FACTOR: usize = 4;

/// One block of the global allocator's memory the arena bumps through.
struct Chunk {
    ptr: NonNull<u8>,
    layout: Layout,
}

impl Chunk {
    /// `size` must be above zero.
    fn try_new(size: usize) -> Result<Self, AllocError> {
        let layout = Layout::from_size_align(size, CHUNK_ALIGN).map_err(|_| AllocError)?;
        let ptr = Global.allocate(layout)?.cast::<u8>();
        Ok(Self { ptr, layout })
    }

    fn capacity(&self) -> usize {
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

/// Scratch memory for one owner's frame walks, handed out by bumping a cursor and freed all at
/// once by [`Self::reset`] (ADR-0328).
///
/// A collection allocates from it through the [`Allocator`] implementation on `&FrameArena`:
/// `Vec::new_in(&arena)`. The arena only provides memory: each collection still drops its own
/// elements, so a destructor never runs out of the arena itself.
///
/// It is neither `Send` nor `Sync`: one owner thread holds it, as it holds the walks that use it.
///
/// A collection that is still alive cannot meet a reset:
///
/// ```compile_fail,E0502
/// use flui_foundation::arena::{FrameArena, FrameVec};
///
/// let mut arena = FrameArena::new();
/// let mut kept: FrameVec<'_, u8> = Vec::new_in(&arena);
/// kept.push(1);
/// arena.reset();
/// kept.push(2);
/// ```
pub struct FrameArena {
    /// Every chunk opened since the last reset; the cursor bumps through the last one.
    chunks: RefCell<Vec<Chunk>>,
    /// The start of the current chunk; dangling while there is none.
    base: Cell<NonNull<u8>>,
    /// The current chunk's size; zero while there is none.
    capacity: Cell<usize>,
    /// Bytes of the current chunk handed out, padding included.
    cursor: Cell<usize>,
    /// Bytes handed out from the chunks before the current one since the last reset.
    retired: Cell<usize>,
    /// The most bytes handed out at once since the last reset: a frame that frees its scratch
    /// in reverse order ends with the cursor back at zero, and still needed this much.
    peak: Cell<usize>,
    /// The size class of each of the last [`RECENT_RESETS`] resets that followed some
    /// allocation: the power of two its peak rounds up to (at least [`MIN_CHUNK`]), as an
    /// exponent, so the window stays small inside the owner that holds the arena; zero for a slot
    /// no reset has written yet.
    recent: [u8; RECENT_RESETS],
    /// Where the next reset writes in `recent`.
    next_recent: usize,
    /// The size the next chunk opens at, so a frame that needed several chunks takes one the
    /// next time: a reset sets it, and never allocates.
    next_chunk: usize,
}

/// The exponent of the power of two `bytes` rounds up to, at least [`MIN_CHUNK`]'s; a size past
/// the largest power of two is class `usize::BITS`.
fn size_class(bytes: usize) -> u8 {
    let class = bytes
        .max(MIN_CHUNK)
        .checked_next_power_of_two()
        .map_or(usize::BITS, usize::trailing_zeros);
    u8::try_from(class).expect("BUG: a size class is at most usize::BITS")
}

/// The bytes of size class `class`; the class past the largest power of two saturates.
fn class_size(class: u8) -> usize {
    1_usize.checked_shl(u32::from(class)).unwrap_or(usize::MAX)
}

impl FrameArena {
    /// An arena that holds no memory until its first allocation.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            chunks: RefCell::new(Vec::new()),
            base: Cell::new(NonNull::dangling()),
            capacity: Cell::new(0),
            cursor: Cell::new(0),
            retired: Cell::new(0),
            peak: Cell::new(0),
            recent: [0; RECENT_RESETS],
            next_recent: 0,
            next_chunk: 0,
        }
    }

    /// Frees every block at once. `&mut self` proves that no collection still borrows the
    /// arena.
    ///
    /// Keeps the newest chunk when it holds the most this frame had handed out at once, so the
    /// next frame of the same shape allocates nothing from the global allocator; releases the
    /// others, and a chunk larger than four times the most any of the last 120 frames that used
    /// the arena needed. Keeping no chunk, it frees the list of chunks too. A frame that handed
    /// out nothing changes nothing. A reset never allocates: when it keeps no chunk, the next
    /// allocation opens one sized for the recent frames.
    pub fn reset(&mut self) {
        let used = self.peak.get();
        if used == 0 {
            // Nothing was handed out since the last reset: a pass with no scratch to list says
            // nothing about how much the next busy one needs, so it neither counts as a recent
            // frame nor releases the chunk those frames sized.
            return;
        }
        self.recent[self.next_recent] = size_class(used);
        self.next_recent = (self.next_recent + 1) % RECENT_RESETS;
        let wanted = self
            .recent
            .iter()
            .copied()
            .max()
            .map_or(MIN_CHUNK, class_size);

        let chunks = self.chunks.get_mut();
        let keep_newest = chunks.last().is_some_and(|chunk| {
            chunk.capacity() >= used && chunk.capacity() <= wanted.saturating_mul(RELEASE_FACTOR)
        });
        let kept = if keep_newest { chunks.pop() } else { None };
        self.next_chunk = wanted;
        if let Some(chunk) = kept {
            chunks.clear();
            self.base.set(chunk.ptr);
            self.capacity.set(chunk.capacity());
            chunks.push(chunk);
        } else {
            // Keeping no chunk, the arena keeps no list of them either: the slots a grown chain
            // left would stay live between frames for nothing.
            *chunks = Vec::new();
            self.base.set(NonNull::dangling());
            self.capacity.set(0);
        }
        self.cursor.set(0);
        self.retired.set(0);
        self.peak.set(0);
    }

    /// Bytes handed out since the last reset, padding included.
    fn used(&self) -> usize {
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
        let offset = start - base;
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
        let chunk = Chunk::try_new(size)?;
        let ptr = chunk.ptr;
        self.chunks.borrow_mut().push(chunk);
        self.retired.set(self.used());
        self.base.set(ptr);
        self.capacity.set(size);
        self.cursor.set(0);
        Ok(())
    }
}

impl Default for FrameArena {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Debug for FrameArena {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Read everything first: the writer is code the arena does not own, and it may allocate
        // from this arena, which opens a chunk under `chunks.borrow_mut()`.
        let chunks = self.chunks.borrow().len();
        let capacity = self.capacity.get();
        let used = self.used();
        f.debug_struct("FrameArena")
            .field("chunks", &chunks)
            .field("capacity", &capacity)
            .field("used", &used)
            .finish_non_exhaustive()
    }
}

/// A dangling pointer aligned for `align`, for a zero-size block.
fn dangling(align: usize) -> NonNull<[u8]> {
    let ptr = NonNull::new(std::ptr::without_provenance_mut::<u8>(align))
        .expect("BUG: a layout's alignment is a non-zero power of two");
    NonNull::slice_from_raw_parts(ptr, 0)
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
            return Ok(dangling(layout.align()));
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
            return Ok(dangling(new_layout.align()));
        }
        if ptr.addr().get().is_multiple_of(new_layout.align()) {
            if let Some(offset) = self.last_block_offset(ptr, old_layout.size()) {
                self.cursor.set(offset + new_layout.size());
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

#[cfg(test)]
mod tests {
    use super::*;

    fn chunk_count(arena: &FrameArena) -> usize {
        arena.chunks.borrow().len()
    }

    /// A frame that frees its scratch in reverse order still keeps the chunk it needed.
    #[test]
    fn a_frame_that_frees_in_order_keeps_its_chunk() {
        let mut arena = FrameArena::new();
        for _ in 0..3 {
            {
                let outer: FrameVec<'_, u8> = Vec::with_capacity_in(100, &arena);
                let inner: FrameVec<'_, u8> = Vec::with_capacity_in(100, &arena);
                drop(inner);
                drop(outer);
                assert_eq!(arena.cursor.get(), 0, "both blocks came back");
            }
            let base = arena.base.get();
            arena.reset();
            assert_eq!(chunk_count(&arena), 1);
            assert_eq!(
                arena.base.get(),
                base,
                "the same chunk serves the next frame"
            );
        }
    }

    #[test]
    fn a_frame_of_the_same_shape_reuses_the_one_kept_chunk() {
        let mut arena = FrameArena::new();
        let mut bases = Vec::new();
        for _ in 0..4 {
            {
                let mut outer: FrameVec<'_, u64> = Vec::new_in(&arena);
                for n in 0..3_000_u64 {
                    outer.push(n);
                }
                let mut inner: FrameVec<'_, String> = Vec::new_in(&arena);
                inner.push(String::from("owned by the global allocator"));
                assert_eq!(inner.len(), 1);
                assert_eq!(outer.iter().sum::<u64>(), 3_000 * 2_999 / 2);
            }
            bases.push((chunk_count(&arena), arena.base.get()));
            arena.reset();
        }
        assert!(
            bases[0].0 > 1,
            "the first frame grew through several chunks"
        );
        assert!(
            bases[1..]
                .iter()
                .all(|&(chunks, base)| chunks == 1 && base == bases[1].1),
            "the reset sized one chunk, and every later frame bumps through it alone",
        );
    }

    #[test]
    fn freeing_or_growing_the_last_block_moves_the_cursor_in_place() {
        let arena = FrameArena::new();
        let mut first: FrameVec<'_, u32> = Vec::with_capacity_in(4, &arena);
        first.extend([1, 2, 3, 4]);
        let at = first.as_ptr();
        first.reserve_exact(60);
        assert_eq!(first.as_ptr(), at, "the last block grows in place");
        assert_eq!(arena.cursor.get(), 64 * 4);

        let second: FrameVec<'_, u32> = Vec::with_capacity_in(8, &arena);
        first.reserve_exact(1_000);
        assert_ne!(first.as_ptr(), at, "a block that is not last moves");
        assert_eq!(&first[..4], [1, 2, 3, 4], "and keeps its contents");
        assert_eq!(second.capacity(), 8, "the block after it is untouched");
    }

    #[test]
    fn a_block_freed_out_of_order_waits_for_the_reset() {
        let arena = FrameArena::new();
        let first: FrameVec<'_, u8> = Vec::with_capacity_in(16, &arena);
        let second: FrameVec<'_, u8> = Vec::with_capacity_in(16, &arena);
        let cursor = arena.cursor.get();
        drop(first);
        assert_eq!(
            arena.cursor.get(),
            cursor,
            "the first block is not the last"
        );
        drop(second);
        assert_eq!(arena.cursor.get(), 16, "the second one is, and frees");
    }

    #[test]
    fn zero_size_and_over_aligned_layouts_are_honoured() {
        #[derive(Clone, Copy)]
        #[repr(align(4096))]
        struct Page(#[expect(dead_code, reason = "the field only gives the type its size")] u8);

        let arena = FrameArena::new();
        let empty: FrameVec<'_, ()> = {
            let mut units = Vec::new_in(&arena);
            units.extend([(), (), ()]);
            units
        };
        assert_eq!(empty.len(), 3);
        assert_eq!(arena.cursor.get(), 0, "zero-size elements take no memory");

        let mut byte: FrameVec<'_, u8> = Vec::new_in(&arena);
        byte.push(1);
        assert_eq!(byte, [1]);
        let mut pages: FrameVec<'_, Page> = Vec::new_in(&arena);
        pages.push(Page(1));
        pages.push(Page(2));
        assert!(
            pages.as_ptr().addr().is_multiple_of(4096),
            "aligned past the chunk"
        );
    }

    #[test]
    fn a_chunk_one_heavy_frame_grew_is_released_after_recent_frames_shrink() {
        let mut arena = FrameArena::new();
        {
            let heavy: FrameVec<'_, u8> = Vec::with_capacity_in(1 << 20, &arena);
            // Freed in order, so the cursor is back at zero when the frame ends.
            drop(heavy);
        }
        arena.reset();
        let heavy_capacity = arena.capacity.get();
        assert!(heavy_capacity >= 1 << 20);

        for frame in 0..RECENT_RESETS {
            {
                let _light: FrameVec<'_, u8> = Vec::with_capacity_in(64, &arena);
            }
            arena.reset();
            if frame + 1 < RECENT_RESETS {
                assert_eq!(
                    arena.capacity.get(),
                    heavy_capacity,
                    "frame {frame}: still recent"
                );
            }
        }
        assert_eq!(chunk_count(&arena), 0, "the heavy frame left the window");
        let mut light: FrameVec<'_, u8> = Vec::new_in(&arena);
        light.push(1);
        assert_eq!(light, [1]);
        assert_eq!(
            arena.capacity.get(),
            MIN_CHUNK,
            "the next chunk is sized for light frames"
        );
    }

    /// A reset releases a chunk that did not hold the frame alone and opens none itself: the next
    /// allocation opens one sized for the whole frame.
    #[test]
    fn a_reset_never_allocates() {
        let mut arena = FrameArena::new();
        {
            // Fills the first chunk exactly, then a block that fills most of the second, so the
            // newest chunk is smaller than the frame's peak.
            let _first: FrameVec<'_, u8> = Vec::with_capacity_in(MIN_CHUNK, &arena);
            assert_eq!(arena.capacity.get(), 2 * MIN_CHUNK);
            let _second: FrameVec<'_, u8> = Vec::with_capacity_in(MIN_CHUNK, &arena);
            let _third: FrameVec<'_, u8> = Vec::with_capacity_in(4 * MIN_CHUNK - 16, &arena);
            assert_eq!(chunk_count(&arena), 2);
        }
        let peak = arena.peak.get();
        assert!(
            peak > arena.capacity.get(),
            "the newest chunk cannot hold the frame"
        );
        arena.reset();
        assert_eq!(
            chunk_count(&arena),
            0,
            "the reset keeps no chunk and opens none"
        );
        assert_eq!(
            arena.chunks.borrow().capacity(),
            0,
            "keeping no chunk, the reset frees the list of chunks"
        );
        assert_eq!(arena.capacity.get(), 0);
        assert!(
            arena.next_chunk >= peak,
            "the next chunk will hold the frame"
        );

        let _next: FrameVec<'_, u8> = Vec::with_capacity_in(1, &arena);
        assert!(
            arena.capacity.get() >= peak,
            "and the next allocation opens it"
        );
    }

    /// Passes that hand out nothing neither age the recent peaks nor release the chunk they sized.
    #[test]
    fn idle_resets_keep_the_chunk_busy_frames_need() {
        let mut arena = FrameArena::new();
        {
            let _busy: FrameVec<'_, u8> = Vec::with_capacity_in(10 * MIN_CHUNK, &arena);
        }
        arena.reset();
        let base = arena.base.get();
        for _ in 0..2 * RECENT_RESETS {
            arena.reset();
        }
        assert_eq!(
            arena.base.get(),
            base,
            "the busy frames' chunk is still held"
        );
        {
            let _busy: FrameVec<'_, u8> = Vec::with_capacity_in(10 * MIN_CHUNK, &arena);
        }
        assert_eq!(
            arena.base.get(),
            base,
            "and the next busy frame bumps through it"
        );
    }

    #[test]
    fn a_shrunk_last_block_returns_its_tail() {
        let arena = FrameArena::new();
        let mut bytes: FrameVec<'_, u8> = Vec::with_capacity_in(256, &arena);
        bytes.extend([7; 10]);
        bytes.shrink_to_fit();
        assert_eq!(arena.cursor.get(), 10);
        assert_eq!(bytes, [7; 10]);
    }

    /// A writer that allocates from the arena it formats opens a chunk while it writes.
    #[test]
    fn formatting_into_the_arena_it_describes_opens_a_chunk() {
        struct Writer<'f>(FrameVec<'f, u8>);
        impl fmt::Write for Writer<'_> {
            fn write_str(&mut self, text: &str) -> fmt::Result {
                self.0.extend_from_slice(text.as_bytes());
                Ok(())
            }
        }
        let arena = FrameArena::new();
        let mut out = Writer(Vec::new_in(&arena));
        fmt::write(&mut out, format_args!("{arena:?}")).expect("formatting succeeds");
        assert!(out.0.starts_with(b"FrameArena { chunks: 0,"));
        assert_eq!(chunk_count(&arena), 1);
    }

    /// The arena lives inline in every pipeline owner, so its window of recent peaks stays a
    /// few hundred bytes.
    #[test]
    fn the_arena_is_small_inline() {
        assert!(
            std::mem::size_of::<FrameArena>() <= 256,
            "{} bytes",
            std::mem::size_of::<FrameArena>()
        );
        assert_eq!(size_class(1), 12);
        assert_eq!(class_size(size_class(MIN_CHUNK + 1)), 2 * MIN_CHUNK);
        assert_eq!(class_size(size_class(usize::MAX)), usize::MAX);
    }

    #[test]
    fn an_empty_arena_holds_no_memory_and_resets() {
        let mut arena = FrameArena::default();
        arena.reset();
        assert_eq!(chunk_count(&arena), 0);
        assert_eq!(arena.capacity.get(), 0);
        let mut one: FrameVec<'_, u8> = Vec::new_in(&arena);
        one.push(1);
        assert_eq!(one, [1]);
        assert_eq!(chunk_count(&arena), 1);
    }
}
