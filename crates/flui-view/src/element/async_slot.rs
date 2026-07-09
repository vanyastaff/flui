//! The shared snapshot slot behind `FutureBuilder` and `StreamBuilder`
//! (ADR-0018, units U4/U5).
//!
//! Both builders publish an [`AsyncSnapshot`] from a spawned task and read it
//! back during `build`. The channel is the same shape as ADR-0017's
//! `LayoutConstraintsCell`: an `Arc<Mutex<…>>` the task writes and the element
//! reads, plus the bookkeeping needed to reject a write from a subscription that
//! has since been replaced.

use std::sync::Arc;

use flui_foundation::AsyncSnapshot;
use parking_lot::Mutex;

use crate::BoxedView;
use crate::context::BuildContext;

/// Produces the initial datum, if any.
///
/// A factory rather than a `T`, so `T` needs no `Clone` to sit inside a view that
/// is cloned on every rebuild.
pub type InitialDataFactory<T> = Arc<dyn Fn() -> T + Send + Sync>;

/// Builds the child from the latest snapshot.
///
/// The snapshot is passed by **reference**, so neither `T` nor `E` needs `Clone`.
pub type SnapshotBuilder<T, E> =
    Arc<dyn Fn(&dyn BuildContext, &AsyncSnapshot<T, E>) -> BoxedView + Send + Sync>;

/// Snapshot plus the bookkeeping a completion needs, shared between the state and
/// the spawned task.
pub(crate) struct Slot<T, E> {
    /// What `build` reads.
    pub(crate) snapshot: AsyncSnapshot<T, E>,

    /// Bumped on every (re)subscription. A write whose generation is stale — the
    /// key changed, or the state was disposed — is discarded.
    ///
    /// Dropping the `TaskToken` already cancels the task, so this is defence in
    /// depth for the window where a task produced a value but its writer has not
    /// yet taken the lock. Flutter's `_activeCallbackIdentity` plays the same
    /// role — and for Dart it is the *only* defence, since a `Future` cannot be
    /// cancelled at all.
    pub(crate) generation: u64,

    /// True only while `AsyncDriver::spawn_local_eager`'s inline poll runs.
    ///
    /// A completion in that window must not schedule a rebuild: the build that
    /// reads it has not run yet, so scheduling would cost a wasted frame. Only
    /// `FutureBuilder` opens this window — `StreamBuilder` never polls inline,
    /// because Dart's `Stream.listen` never delivers an event synchronously.
    pub(crate) inline_window: bool,
}

impl<T, E> Slot<T, E> {
    /// A slot holding `snapshot` at generation 0, outside any inline window.
    pub(crate) fn new(snapshot: AsyncSnapshot<T, E>) -> Self {
        Self {
            snapshot,
            generation: 0,
            inline_window: false,
        }
    }

    /// Replace the snapshot by folding the current one through `fold`.
    ///
    /// Takes the snapshot out and puts the result back, so `fold` can consume it
    /// — which is what every `AsyncSnapshot` transition does, and why neither `T`
    /// nor `E` needs `Clone`.
    pub(crate) fn fold(&mut self, fold: impl FnOnce(AsyncSnapshot<T, E>) -> AsyncSnapshot<T, E>) {
        let snapshot = core::mem::replace(&mut self.snapshot, AsyncSnapshot::nothing());
        self.snapshot = fold(snapshot);
    }
}

/// Shared handle to the snapshot.
pub(crate) type SharedSlot<T, E> = Arc<Mutex<Slot<T, E>>>;

/// Fold `fold` into the shared snapshot, unless the write is stale.
///
/// Returns whether a rebuild must be scheduled: `false` for a stale write (the
/// subscription was replaced or disposed — do not wake a frame for a snapshot
/// nobody will read) and `false` inside the inline window (the build that will
/// read it has not happened yet).
///
/// Split out of the spawned tasks so the generation guard is directly testable.
/// Through the builders it is unreachable — dropping the `TaskToken` cancels the
/// task before its writer can run — but a leaked token, or a future/stream that
/// resolves on another thread, would reopen the window.
pub(crate) fn apply_fold<T, E>(
    slot: &SharedSlot<T, E>,
    generation: u64,
    fold: impl FnOnce(AsyncSnapshot<T, E>) -> AsyncSnapshot<T, E>,
) -> bool {
    let mut slot = slot.lock();

    if slot.generation != generation {
        return false;
    }

    slot.fold(fold);
    !slot.inline_window
}
