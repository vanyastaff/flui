//! The shared snapshot slot behind `FutureBuilder` and `StreamBuilder`
//! (ADR-0018).
//!
//! Both builders publish an [`AsyncSnapshot`] from a spawned task and read it
//! back during `build`. The channel is an `Rc<RefCell<…>>` on the owner
//! thread (the task is a local task of the binding's driver): the task writes
//! it and the element reads it, with the bookkeeping that rejects a write from a
//! subscription that has since been replaced.

use std::{cell::RefCell, rc::Rc};

use super::{AsyncSnapshot, ConnectionState};
use crate::RebuildHandle;
use flui_foundation::panic::FirstPanic;

use crate::BoxedView;
use crate::context::BuildContext;

/// Produces the initial datum, if any.
///
/// A factory rather than a `T`, so `T` needs no `Clone` to sit inside a view that
/// is cloned on every rebuild.
pub type InitialDataFactory<T> = Rc<dyn Fn() -> T>;

/// Builds the child from the latest snapshot.
///
/// The snapshot is passed by **reference**, so neither `T` nor `E` needs `Clone`.
pub type SnapshotBuilder<T, E> = Rc<dyn Fn(&dyn BuildContext, &AsyncSnapshot<T, E>) -> BoxedView>;

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
    /// yet written the slot.
    pub(crate) generation: u64,

    /// True only while `AsyncDriver::spawn_local_eager`'s inline poll runs.
    ///
    /// A completion in that window must not schedule a rebuild: the build that
    /// reads it has not run yet, so scheduling would cost a wasted frame. Only
    /// `FutureBuilder` opens this window — `StreamBuilder` never polls inline,
    /// because a stream subscription never delivers an event synchronously.
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

    /// Publish only a connection-state change, preserving owned data/error.
    pub(crate) fn set_connection_state(&mut self, state: ConnectionState) {
        let snapshot = core::mem::take(&mut self.snapshot);
        self.snapshot = snapshot.in_state(state);
    }
}

/// Shared handle to the snapshot.
pub(crate) type SharedSlot<T, E> = Rc<RefCell<Slot<T, E>>>;

/// Closed updates used by the builders: replacement or payload-preserving state.
pub(crate) enum SnapshotUpdate<T, E> {
    Replace(AsyncSnapshot<T, E>),
    State(ConnectionState),
}

/// Publish under the subscription fence, then retire and request a rebuild
/// outside the slot borrow. Returns whether the subscription accepted the update.
pub(crate) fn apply_update<T, E>(
    slot: &SharedSlot<T, E>,
    generation: u64,
    update: SnapshotUpdate<T, E>,
    handle: &RebuildHandle,
) -> bool {
    let (accepted, rebuild, retired) = {
        let mut slot = slot.borrow_mut();
        if slot.generation == generation {
            let previous = match update {
                SnapshotUpdate::Replace(snapshot) => {
                    Some(core::mem::replace(&mut slot.snapshot, snapshot))
                }
                SnapshotUpdate::State(state) => {
                    slot.set_connection_state(state);
                    None
                }
            };
            (true, !slot.inline_window, previous)
        } else {
            let rejected = match update {
                SnapshotUpdate::Replace(snapshot) => Some(snapshot),
                SnapshotUpdate::State(_) => None,
            };
            (false, false, rejected)
        }
    };
    // The incoming value belongs to the published slot already. Retiring the
    // old snapshot cannot destroy it during the old value's unwind.
    // A retired snapshot's panic does not lose the rebuild the published
    // value owes.
    let mut first = FirstPanic::new();
    first.contain(|| drop(retired));
    if rebuild {
        first.contain(|| handle.schedule(crate::RebuildReason::AsyncCompletion));
    }
    first.resume();
    accepted
}
