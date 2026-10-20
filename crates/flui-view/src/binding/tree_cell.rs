//! The binding's owner-thread cell, which answers a busy tree instead of deadlocking.

use std::cell::{Cell, Ref, RefCell, RefMut};
use std::fmt;
use std::ops::{Deref, DerefMut};

use flui_foundation::inspect::SnapshotError;

/// What the binding was doing with its tree when a call reached it again.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub(crate) enum TreePhase {
    /// Building or tearing down elements, so a view's `build` or a lifecycle hook is running.
    Building,
    /// Taking a snapshot, so a view's `inspect` is running.
    Inspecting,
    /// Lending the tree out for reading, as `with_element_tree` does.
    Reading,
    /// Lending the tree out for writing, as `with_element_tree_mut` does.
    Writing,
}

impl TreePhase {
    /// The phase as the end of "the element tree is busy …".
    #[must_use]
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Building => "building",
            Self::Inspecting => "inspecting",
            Self::Reading => "lent for reading",
            Self::Writing => "lent for writing",
        }
    }
}

impl fmt::Display for TreePhase {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// The element tree is already in use further up this thread's stack.
///
/// The binding lives on its owner thread, so a tree that is in use is in use by the caller's
/// own stack: code that runs inside a build, a snapshot or a `with_element_tree` closure has
/// reached back into its binding. Waiting would never end, so a snapshot answers this as
/// [`SnapshotError::Busy`] and every other call panics with it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
#[error(
    "the element tree is busy {phase}: code running inside the binding reached back into it; \
     defer the work (mark the element dirty, schedule a callback)"
)]
pub(crate) struct TreeBusy {
    /// What the binding was doing.
    pub(crate) phase: TreePhase,
}

impl From<TreeBusy> for SnapshotError {
    fn from(busy: TreeBusy) -> Self {
        Self::Busy(busy.phase.as_str())
    }
}

/// A `RefCell` that names what it is busy with.
pub(super) struct TreeCell<T> {
    value: RefCell<T>,
    phase: Cell<Option<TreePhase>>,
}

impl<T> TreeCell<T> {
    pub(super) const fn new(value: T) -> Self {
        Self {
            value: RefCell::new(value),
            phase: Cell::new(None),
        }
    }

    /// Shared access, or the busy phase while an exclusive borrow is live.
    pub(super) fn try_read(&self) -> Result<Ref<'_, T>, TreeBusy> {
        self.value
            .try_borrow()
            .map_err(|_| self.busy(TreePhase::Writing))
    }

    /// Exclusive access, or the busy phase while any borrow is live.
    pub(super) fn try_write(&self) -> Result<RefMut<'_, T>, TreeBusy> {
        self.value.try_borrow_mut().map_err(|_| {
            let lent = if self.value.try_borrow().is_ok() {
                TreePhase::Reading
            } else {
                TreePhase::Writing
            };
            self.busy(lent)
        })
    }

    /// [`Self::try_read`] for calls with no error path, which a busy tree makes a caller error.
    #[track_caller]
    pub(super) fn read(&self) -> Ref<'_, T> {
        self.try_read().unwrap_or_else(|busy| panic!("{busy}"))
    }

    /// [`Self::try_write`] for calls with no error path, which a busy tree makes a caller error.
    #[track_caller]
    pub(super) fn write(&self) -> RefMut<'_, T> {
        self.try_write().unwrap_or_else(|busy| panic!("{busy}"))
    }

    /// [`Self::write`] that names `phase` while the borrow lives, so a call reaching back in
    /// is told what the binding is doing rather than only that the tree is lent.
    #[track_caller]
    pub(super) fn write_in(&self, phase: TreePhase) -> PhasedMut<'_, T> {
        let value = self.write();
        PhasedMut {
            value,
            _phase: self.enter(phase),
        }
    }

    /// [`Self::try_read`] that names `phase` while the borrow lives. Shared borrows nest, so
    /// this also refuses while any phase is named: a snapshot asked for from a view's `inspect`
    /// would otherwise walk the tree again from inside its own walk.
    pub(super) fn try_read_in(&self, phase: TreePhase) -> Result<PhasedRef<'_, T>, TreeBusy> {
        if let Some(phase) = self.phase.get() {
            return Err(TreeBusy { phase });
        }
        let value = self.try_read()?;
        Ok(PhasedRef {
            value,
            _phase: self.enter(phase),
        })
    }

    fn busy(&self, lent: TreePhase) -> TreeBusy {
        TreeBusy {
            phase: self.phase.get().unwrap_or(lent),
        }
    }

    /// Names what the tree is busy with until the returned guard drops, unwinding included.
    /// The outermost phase is the one a busy answer reports.
    fn enter(&self, phase: TreePhase) -> PhaseGuard<'_> {
        let previous = self.phase.get();
        if previous.is_none() {
            self.phase.set(Some(phase));
        }
        PhaseGuard {
            phase: &self.phase,
            previous,
        }
    }
}

/// Restores the phase that was named before [`TreeCell::enter`].
struct PhaseGuard<'a> {
    phase: &'a Cell<Option<TreePhase>>,
    previous: Option<TreePhase>,
}

impl Drop for PhaseGuard<'_> {
    fn drop(&mut self) {
        self.phase.set(self.previous);
    }
}

/// Exclusive access with its phase named; [`TreeCell::write_in`].
pub(super) struct PhasedMut<'a, T> {
    value: RefMut<'a, T>,
    _phase: PhaseGuard<'a>,
}

impl<T> Deref for PhasedMut<'_, T> {
    type Target = T;

    fn deref(&self) -> &T {
        &self.value
    }
}

impl<T> DerefMut for PhasedMut<'_, T> {
    fn deref_mut(&mut self) -> &mut T {
        &mut self.value
    }
}

/// Shared access with its phase named; [`TreeCell::try_read_in`].
pub(super) struct PhasedRef<'a, T> {
    value: Ref<'a, T>,
    _phase: PhaseGuard<'a>,
}

impl<T> Deref for PhasedRef<'_, T> {
    type Target = T;

    fn deref(&self) -> &T {
        &self.value
    }
}
