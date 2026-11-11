//! Owner-local vectors a walk checks out and gives back, so a frame reuses
//! the allocations of the frames before it.

use std::cell::RefCell;
use std::ops::{Deref, DerefMut};

use smallvec::SmallVec;

/// A buffer that holds more than this many bytes is freed instead of kept:
/// one wide parent does not pin its child list for the owner's life.
const MAX_RETAINED_BYTES: usize = 16 * 1024;

/// At most this many idle buffers are kept, one per level of the deepest
/// walk that ran.
const MAX_RETAINED_BUFFERS: usize = 256;

/// Idle buffers kept without a heap list of their own.
const IDLE_INLINE: usize = 8;

/// Idle vectors of `T`, owned by one `PipelineOwner`.
///
/// A checkout owns its vector outright, so a reentrant walk (a child's
/// layout under its parent's, a transform query from an authored transform)
/// takes a different one and no borrow of the pool spans code it does not
/// own. Every exit, unwinding included, gives the allocation back.
pub(super) struct BufferPool<T> {
    /// Inline for the shallow walks most frames run; a deeper one spills
    /// once and keeps the spill.
    idle: RefCell<SmallVec<[Vec<T>; IDLE_INLINE]>>,
}

impl<T> Default for BufferPool<T> {
    fn default() -> Self {
        Self {
            idle: RefCell::new(SmallVec::new()),
        }
    }
}

impl<T> BufferPool<T> {
    /// An empty vector that holds at least `capacity` items, reusing an idle
    /// allocation when there is one. A fresh one is sized exactly, not to
    /// `Vec`'s minimum growth step.
    pub(super) fn checkout(&self, capacity: usize) -> Pooled<'_, T> {
        let mut items = self.idle.borrow_mut().pop().unwrap_or_default();
        items.reserve_exact(capacity);
        Pooled { pool: self, items }
    }
}

/// A vector checked out of a [`BufferPool`]; dropping it gives it back.
pub(super) struct Pooled<'a, T> {
    pool: &'a BufferPool<T>,
    items: Vec<T>,
}

impl<T> Deref for Pooled<'_, T> {
    type Target = Vec<T>;

    fn deref(&self) -> &Vec<T> {
        &self.items
    }
}

impl<T> DerefMut for Pooled<'_, T> {
    fn deref_mut(&mut self) -> &mut Vec<T> {
        &mut self.items
    }
}

impl<T> Drop for Pooled<'_, T> {
    fn drop(&mut self) {
        let mut items = std::mem::take(&mut self.items);
        // The items' own `Drop` runs here, before the pool is borrowed.
        items.clear();
        let bytes = items.capacity().saturating_mul(std::mem::size_of::<T>());
        if items.capacity() == 0 || bytes > MAX_RETAINED_BYTES {
            return;
        }
        let mut idle = self.pool.idle.borrow_mut();
        if idle.len() < MAX_RETAINED_BUFFERS {
            idle.push(items);
        }
    }
}
