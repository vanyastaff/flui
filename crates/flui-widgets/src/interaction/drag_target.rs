//! [`DragTarget`] — receives typed data when a [`Draggable`](crate::Draggable)
//! is dropped on it.
//!
//! Flutter parity: `widgets/drag_target.dart` (tag `3.44.0`) — `DragTarget`,
//! `_DragTargetState`, `DragTargetDetails`. This is the accept/candidate/
//! reject/leave state machine, and it is now **live**: the target publishes a
//! shared [`DragTargetSlot`] as its hit-test payload (through
//! [`MetaData`]), and a dragging [`Draggable`](crate::Draggable)
//! discovers it by hit-testing at the pointer's current global position on
//! every move — the oracle's `_DragAvatar.updateDrag` / `_getDragTargets`
//! shape. See [`crate::Draggable`]'s module docs for the discovery half.
//!
//! # Divergences from the oracle
//!
//! - **The transitions live on a shared slot, not on the state object.** The
//!   oracle's `_getDragTargets` finds a `_DragTargetState` on the hit path and
//!   calls `didEnter`/`didMove`/`didLeave`/`didDrop` on it directly, because
//!   Dart's payload is a GC'd reference to the live `State` and that `State`
//!   can reach its own widget for the callbacks. Neither holds here: a
//!   hit-test payload is `Arc<dyn Any + Send + Sync>`, and FLUI's callbacks
//!   live on the *view*, which the state does not own. So the payload is an
//!   `Arc<DragTargetSlot>` that carries both the entered list and the
//!   callbacks; each build refreshes the callbacks into it, and
//!   [`DragTargetState`] reads its candidate/rejected lists back out of it.
//!   Recorded in `crates/flui-widgets/ARCHITECTURE.md` (`## Mapping decisions`).
//! - **Callbacks are `Arc<dyn Fn … + Send + Sync>`, not `Rc<dyn Fn …>`.**
//!   Forced by the same payload bound, and it makes `DragTarget` consistent
//!   with [`Draggable`](crate::Draggable), whose callbacks already were.
//!   The *builder* stays `Rc`: it produces a `BoxedView`, which is
//!   owner-local by construction.
//! - **`DragTargetDetails` also carries a target-local position.** The
//!   oracle's `DragTargetDetails.offset` is a global position and nothing
//!   else; a Dart target that wants a local one calls `globalToLocal` on its
//!   own render object, which FLUI callback code cannot reach. So `offset`
//!   keeps the oracle's global meaning and
//!   [`local_offset`](DragTargetDetails::local_offset) adds the same point
//!   mapped through the hit entry's own global-to-local transform — correct
//!   under transforms and nested scroll offsets, where subtracting a
//!   remembered origin is not.
//! - **One accept callback, not two.** The oracle carries both the deprecated
//!   `onWillAccept`/`onAccept` (data-only) and the current
//!   `onWillAcceptWithDetails`/`onAcceptWithDetails` (details-carrying) pairs,
//!   asserting the two forms of each are not combined. FLUI ships only the
//!   details-carrying form under the plain name (`on_will_accept`,
//!   `on_accept`) — there is no deprecated predecessor to stay compatible
//!   with in a new port.
//! - **`rejected_data` is typed (`&[T]`), not `List<dynamic>`.** The oracle's
//!   `rejectedData` signature is `List<dynamic>`, but `_getDragTargets`
//!   (`drag_target.dart`) filters every hit-tested target by
//!   `isExpectedDataType(data, T)` *before* `didEnter` is ever called for it
//!   — a type-mismatched drag never becomes an entry in `_rejectedAvatars`
//!   (or `_candidateAvatars`) at all, only an `onWillAccept`-vetoed drag
//!   whose data already matched `T` does. So the oracle's own rejected list,
//!   for a given `DragTarget<T>`, only ever holds `T?`-typed values in
//!   practice — `List<dynamic>` is Dart's loose typing describing a fact
//!   that is always `T`-shaped, not evidence of real heterogeneity. FLUI's
//!   `rejected_data() -> Vec<T>` makes that already-true fact explicit in
//!   the type system rather than replicating Dart's looser surface.
//!   [`DragTargetSlot::did_enter`] mirrors the same discovery-time filter: a
//!   genuinely type-mismatched payload is never added to either list (see
//!   its own doc), so `did_leave`/`did_move` never need to reconstruct a
//!   "was this ever a real `T`" answer after the fact.
//! - **`hit_test_behavior` is not configurable.** The target always tags
//!   itself `HitTestBehavior::Translucent`, which is the oracle's own
//!   default — found within its own bounds without stopping targets beneath
//!   it from being found too, which is what makes overlapping targets
//!   discoverable at all. Making it configurable is a named deferral.

use std::any::Any;
use std::marker::PhantomData;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use flui_interaction::PointerId;
use flui_rendering::hit_testing::HitTestBehavior;
use flui_types::{Offset, geometry::Pixels};
use flui_view::RebuildHandle;
use flui_view::prelude::*;
use parking_lot::Mutex;

use crate::MetaData;

/// A drag's data, type-erased at the `Draggable`/`DragTarget` boundary so a
/// target can reject a payload whose concrete type does not match `T`
/// (`_DragTargetState.isExpectedDataType`), mirroring Dart's `data is T?`.
pub type ErasedDragData = Arc<dyn Any + Send + Sync>;

/// Where a drag currently is, as one particular target sees it.
///
/// Both halves are needed and neither is derivable from the other by the
/// callback: `global` is the pointer's position in the root coordinate space
/// (the oracle's `_lastOffset`), and `local` is that same point mapped into
/// the target's own space through the hit entry's global-to-local transform,
/// so it stays correct under an ancestor `Transform`, a scroll offset, or any
/// other non-translation mapping.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DragPosition {
    /// The pointer position in the global (root) coordinate space.
    pub global: Offset<Pixels>,
    /// The same position in the target's own local coordinate space.
    pub local: Offset<Pixels>,
}

impl DragPosition {
    /// A position whose local space *is* the global space — for a target with
    /// no transform between it and the root, and for direct callers driving
    /// the protocol without a hit-test path.
    #[must_use]
    pub fn global_only(global: Offset<Pixels>) -> Self {
        Self {
            global,
            local: global,
        }
    }
}

/// Details for a [`DragTarget`] callback: the (typed) data and where the drag
/// is.
///
/// Flutter parity: `DragTargetDetails<T>`, plus
/// [`local_offset`](Self::local_offset) — see the module docs.
#[derive(Debug, Clone)]
pub struct DragTargetDetails<T> {
    /// The data carried by the drag.
    pub data: T,
    /// The global position at which the event occurred.
    pub offset: Offset<Pixels>,
    /// The same position in this target's own local coordinate space.
    pub local_offset: Offset<Pixels>,
}

/// Builds a [`DragTarget`]'s contents from its current candidate/rejected
/// state.
///
/// Flutter parity: `DragTargetBuilder<T>`, minus the `BuildContext` parameter
/// (the target's own `build` already has one available if the builder needs
/// ambient lookups — the candidate/rejected data is what changes per drag),
/// and a typed `&[T]` rejected list rather than `List<dynamic>` — see the
/// module docs on why that is a faithful narrowing, not a divergence.
///
/// Stays `Rc` where the transition callbacks became `Arc`: a builder produces
/// a `BoxedView`, which is owner-local, so no `Send + Sync` bound is
/// satisfiable here and none is needed — the builder is only ever called from
/// `build`, on the owner thread.
pub type DragTargetBuilder<T> = Rc<dyn Fn(&[Option<T>], &[T]) -> BoxedView>;

/// Determines whether a [`DragTarget`] will accept `details`.
pub type DragTargetWillAccept<T> = Arc<dyn Fn(&DragTargetDetails<T>) -> bool + Send + Sync>;
/// Fired when an accepted drop lands.
pub type DragTargetAccept<T> = Arc<dyn Fn(DragTargetDetails<T>) + Send + Sync>;
/// Fired when a candidate or rejected drag leaves the target.
pub type DragTargetLeave<T> = Arc<dyn Fn(Option<T>) + Send + Sync>;
/// Fired on every move while a drag is over the target (candidate or not).
pub type DragTargetMove<T> = Arc<dyn Fn(DragTargetDetails<T>) + Send + Sync>;

/// A widget that receives data when a [`Draggable`](crate::Draggable) is
/// dropped on it.
///
/// Flutter parity: `widgets/drag_target.dart` `DragTarget`.
#[derive(Clone, StatefulView)]
pub struct DragTarget<T: Clone + Send + Sync + 'static> {
    builder: DragTargetBuilder<T>,
    on_will_accept: Option<DragTargetWillAccept<T>>,
    on_accept: Option<DragTargetAccept<T>>,
    on_leave: Option<DragTargetLeave<T>>,
    on_move: Option<DragTargetMove<T>>,
}

impl<T: Clone + Send + Sync + 'static> std::fmt::Debug for DragTarget<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DragTarget")
            .field("has_on_will_accept", &self.on_will_accept.is_some())
            .field("has_on_accept", &self.on_accept.is_some())
            .field("has_on_leave", &self.on_leave.is_some())
            .field("has_on_move", &self.on_move.is_some())
            .finish_non_exhaustive()
    }
}

impl<T: Clone + Send + Sync + 'static> DragTarget<T> {
    /// A target whose contents are built from the current candidate/rejected
    /// state.
    pub fn new(builder: impl Fn(&[Option<T>], &[T]) -> BoxedView + 'static) -> Self {
        Self {
            builder: Rc::new(builder),
            on_will_accept: None,
            on_accept: None,
            on_leave: None,
            on_move: None,
        }
    }

    /// Called when a drag enters the target; the returned `bool` decides
    /// candidate (`true`) vs. rejected (`false`).
    #[must_use]
    pub fn on_will_accept(
        mut self,
        callback: impl Fn(&DragTargetDetails<T>) -> bool + Send + Sync + 'static,
    ) -> Self {
        self.on_will_accept = Some(Arc::new(callback));
        self
    }

    /// Called when an accepted drag is dropped on the target.
    #[must_use]
    pub fn on_accept(
        mut self,
        callback: impl Fn(DragTargetDetails<T>) + Send + Sync + 'static,
    ) -> Self {
        self.on_accept = Some(Arc::new(callback));
        self
    }

    /// Called when a candidate or rejected drag leaves the target.
    #[must_use]
    pub fn on_leave(mut self, callback: impl Fn(Option<T>) + Send + Sync + 'static) -> Self {
        self.on_leave = Some(Arc::new(callback));
        self
    }

    /// Called on every move while a drag (candidate or not) is over the
    /// target.
    #[must_use]
    pub fn on_move(
        mut self,
        callback: impl Fn(DragTargetDetails<T>) + Send + Sync + 'static,
    ) -> Self {
        self.on_move = Some(Arc::new(callback));
        self
    }
}

/// `data` as a `T`, or `None` when its concrete type is something else.
///
/// The single place the `Draggable`/`DragTarget` boundary's erasure is
/// reversed. Flutter parity: `_DragTargetState.isExpectedDataType`, i.e. Dart's
/// `data is T?` — a mismatch is a routine answer (the target is filtered out of
/// the drag's discovery), never an error and never a panic.
fn typed_as<T: Clone + Send + Sync + 'static>(data: &ErasedDragData) -> Option<T> {
    let payload = Arc::clone(data);
    payload.downcast::<T>().ok().map(|typed| (*typed).clone()) // PORT-CHECK-OK-DOWNCAST: reverses this boundary's own erasure; see this function's doc.
}

/// One `DragTarget<T>`'s callbacks as a *drag* sees them.
///
/// A drag carries one erased payload and walks a hit path of targets whose
/// `T`s it cannot name, so the per-target `T` has to be discharged on the
/// target's side of the boundary. Each method takes the erased data and
/// downcasts once, here, where `T` is still in scope.
trait TargetCallbacks: Send + Sync {
    /// Whether `data`'s concrete type is this target's `T`
    /// (`_DragTargetState.isExpectedDataType`).
    fn accepts_data_type(&self, data: &ErasedDragData) -> bool;
    /// The `on_will_accept` veto. `true` (candidate) when unset.
    fn will_accept(&self, data: &ErasedDragData, at: DragPosition) -> bool;
    fn leave(&self, data: &ErasedDragData);
    fn moved(&self, data: &ErasedDragData, at: DragPosition);
    fn accept(&self, data: &ErasedDragData, at: DragPosition);
}

/// One target's callbacks, shared between its element and every drag that has
/// discovered it.
type SharedTargetCallbacks = Arc<dyn TargetCallbacks>; // PORT-CHECK-OK-DYN: a drag drives targets whose `T` it cannot name — see `TargetCallbacks`.

/// The `T`-typed side of [`TargetCallbacks`]: one snapshot of a
/// `DragTarget<T>`'s four callbacks, refreshed into the slot on every build so
/// a transition always invokes the *current* view's closures.
// The fields mirror `DragTarget`'s public builder names one-for-one, which is
// what makes the mapping between the two obvious; renaming them to shed the
// shared prefix would trade that for nothing.
#[expect(
    clippy::struct_field_names,
    reason = "mirrors DragTarget's public callback names"
)]
struct TypedCallbacks<T: Clone + Send + Sync + 'static> {
    on_will_accept: Option<DragTargetWillAccept<T>>,
    on_accept: Option<DragTargetAccept<T>>,
    on_leave: Option<DragTargetLeave<T>>,
    on_move: Option<DragTargetMove<T>>,
}

impl<T: Clone + Send + Sync + 'static> TypedCallbacks<T> {
    fn from_view(view: &DragTarget<T>) -> Self {
        Self {
            on_will_accept: view.on_will_accept.clone(),
            on_accept: view.on_accept.clone(),
            on_leave: view.on_leave.clone(),
            on_move: view.on_move.clone(),
        }
    }

    fn details(data: &ErasedDragData, at: DragPosition) -> Option<DragTargetDetails<T>> {
        Some(DragTargetDetails {
            data: typed_as(data)?,
            offset: at.global,
            local_offset: at.local,
        })
    }
}

impl<T: Clone + Send + Sync + 'static> TargetCallbacks for TypedCallbacks<T> {
    fn accepts_data_type(&self, data: &ErasedDragData) -> bool {
        data.is::<T>()
    }

    fn will_accept(&self, data: &ErasedDragData, at: DragPosition) -> bool {
        let Some(callback) = &self.on_will_accept else {
            return true;
        };
        // A payload that is not a `T` never reaches here (discovery filters
        // it), so an absent detail can only mean a caller drove the protocol
        // past that filter — refuse rather than inventing a value.
        Self::details(data, at).is_some_and(|details| callback(&details))
    }

    fn leave(&self, data: &ErasedDragData) {
        if let Some(callback) = &self.on_leave {
            callback(typed_as(data));
        }
    }

    fn moved(&self, data: &ErasedDragData, at: DragPosition) {
        if let Some(callback) = &self.on_move
            && let Some(details) = Self::details(data, at)
        {
            callback(details);
        }
    }

    fn accept(&self, data: &ErasedDragData, at: DragPosition) {
        if let Some(callback) = &self.on_accept
            && let Some(details) = Self::details(data, at)
        {
            callback(details);
        }
    }
}

/// One pointer's standing with a target: the erased data plus whether
/// `on_will_accept` made it a candidate.
///
/// `accepted == false` is exactly the oracle's `_rejectedAvatars`: an
/// `on_will_accept`-vetoed drag whose data already matched `T`, not a
/// foreign-typed one (which never becomes an entry at all).
struct EnteredDrag {
    pointer: PointerId,
    data: ErasedDragData,
    accepted: bool,
}

/// The live handle a [`DragTarget`] publishes to hit tests, and the object a
/// drag drives its transitions through.
///
/// It exists because the two halves of the oracle's `_DragTargetState` cannot
/// travel together in FLUI: a hit-test payload is `Arc<dyn Any + Send + Sync>`
/// and the callbacks live on the view. The slot owns the entered list, holds
/// the current build's callbacks, and knows how to schedule the target's
/// rebuild — so a drag that finds one on a hit path can run the whole
/// protocol against it without ever naming the target's `T` or touching the
/// element tree.
///
/// Shared by `Arc`, and deliberately outliving its element: a drag that has
/// entered a target keeps the slot alive, so a target unmounting mid-drag
/// leaves the drag with a valid — if retired — handle instead
/// of a dangling one. A retired slot answers every transition as a no-op,
/// which is the oracle's `if (!mounted) return;` guard in a form that cannot
/// be forgotten at one call site.
pub struct DragTargetSlot {
    /// The current build's callbacks. Cloned out before every invocation, so
    /// no user code ever runs with this lock held.
    callbacks: Mutex<SharedTargetCallbacks>,
    /// Every drag currently over this target, keyed by pointer so several
    /// simultaneous drags stay independent (`_candidateAvatars` /
    /// `_rejectedAvatars`, which the oracle keys by avatar identity).
    entered: Mutex<Vec<EnteredDrag>>,
    /// The target element's rebuild capability, published by
    /// `DragTargetState::init_state` — never from `build` (port-check trigger
    /// #22). Stands in for the oracle's `setState`.
    rebuild: Mutex<Option<RebuildHandle>>,
    /// `false` once the target's element is disposed.
    mounted: AtomicBool,
}

impl std::fmt::Debug for DragTargetSlot {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DragTargetSlot")
            .field("entered", &self.entered.lock().len())
            .field("mounted", &self.mounted.load(Ordering::Acquire))
            .finish_non_exhaustive()
    }
}

impl DragTargetSlot {
    fn new(callbacks: SharedTargetCallbacks) -> Self {
        // PORT-CHECK-OK-DYN: constructor for the field above.
        Self {
            callbacks: Mutex::new(callbacks),
            entered: Mutex::new(Vec::new()),
            rebuild: Mutex::new(None),
            mounted: AtomicBool::new(true),
        }
    }

    fn set_callbacks(&self, callbacks: SharedTargetCallbacks) {
        // PORT-CHECK-OK-DYN: per-build refresh of the field above.
        *self.callbacks.lock() = callbacks;
    }

    fn publish_rebuild(&self, handle: RebuildHandle) {
        *self.rebuild.lock() = Some(handle);
    }

    /// The target's element has gone. Every later transition is a no-op.
    fn retire(&self) {
        self.mounted.store(false, Ordering::Release);
    }

    /// Clone the callbacks out from under the lock, so user code never runs
    /// while this slot holds one.
    fn callbacks(&self) -> SharedTargetCallbacks {
        // PORT-CHECK-OK-DYN: reader for the field above.
        Arc::clone(&self.callbacks.lock())
    }

    /// The oracle's `setState`: the candidate/rejected lists the builder reads
    /// just changed.
    fn schedule_rebuild(&self) {
        // Cloned out and the lock dropped before calling into the framework.
        let handle = self.rebuild.lock().clone();
        if let Some(handle) = handle {
            handle.schedule(flui_view::RebuildReason::StateChange);
        }
    }

    /// Whether this target's `T` is `data`'s concrete type — the oracle's
    /// `isExpectedDataType`, which `_getDragTargets` applies to filter the hit
    /// path *before* any transition runs.
    #[must_use]
    pub fn accepts_data_type(&self, data: &ErasedDragData) -> bool {
        self.mounted.load(Ordering::Acquire) && self.callbacks().accepts_data_type(data)
    }

    /// A drag identified by `pointer` enters this target carrying `data` at
    /// `at`. Returns whether the target will accept it (candidate) or not
    /// (rejected).
    ///
    /// A `data` whose concrete type does not match this target's `T` is never
    /// tracked at all — no candidate entry, no rejected entry, and returns
    /// `false` without creating anything for `pointer` to leave later. This
    /// mirrors `_getDragTargets`' `isExpectedDataType` filter, which runs
    /// *before* `didEnter` and keeps a type-mismatched avatar out of
    /// `_enteredTargets` entirely — `didEnter` itself, once reached, only ever
    /// decides candidate vs. rejected for already-`T`-typed data via
    /// `on_will_accept`.
    ///
    /// Flutter parity: `_DragTargetState.didEnter`.
    pub fn did_enter(&self, pointer: PointerId, data: &ErasedDragData, at: DragPosition) -> bool {
        if !self.mounted.load(Ordering::Acquire) {
            return false;
        }
        debug_assert!(
            !self.entered.lock().iter().any(|e| e.pointer == pointer),
            "BUG: did_enter called twice for the same pointer without an intervening did_leave"
        );
        let callbacks = self.callbacks();
        if !callbacks.accepts_data_type(data) {
            // Type mismatch: never becomes an entry, matching the oracle's
            // discovery-time filter — no candidate, no rejected, no future
            // did_leave/did_move/did_drop call for this pointer at all.
            return false;
        }
        let accepted = callbacks.will_accept(data, at);
        self.entered.lock().push(EnteredDrag {
            pointer,
            data: Arc::clone(data),
            accepted,
        });
        self.schedule_rebuild();
        accepted
    }

    /// `pointer`'s drag leaves this target — removed from whichever list it
    /// was in, then `on_leave` fires with its data. A no-op for a pointer that
    /// was never tracked (a type mismatch at [`did_enter`](Self::did_enter),
    /// or a repeat call).
    ///
    /// The removal happens even for a retired slot, so "this pointer is no
    /// longer entered" holds unconditionally after this returns; only the
    /// callback is gated, which is the oracle's `if (!mounted) return;`.
    ///
    /// Flutter parity: `_DragTargetState.didLeave`.
    pub fn did_leave(&self, pointer: PointerId) {
        let removed = {
            let mut entered = self.entered.lock();
            entered
                .iter()
                .position(|e| e.pointer == pointer)
                .map(|index| entered.remove(index))
        };
        let Some(removed) = removed else {
            return;
        };
        if !self.mounted.load(Ordering::Acquire) {
            return;
        }
        self.schedule_rebuild();
        self.callbacks().leave(&removed.data);
    }

    /// `pointer`'s drag moves while over this target — fires `on_move` for
    /// **either** standing (candidate or rejected), matching the oracle's
    /// `didMove`, whose only gate is `avatar.data == null` (a genuinely null
    /// payload, not rejection status: a vetoed-but-typed avatar still sits in
    /// `_enteredTargets` and receives moves). A no-op only for an untracked
    /// pointer, or a retired slot.
    ///
    /// Flutter parity: `_DragTargetState.didMove`.
    pub fn did_move(&self, pointer: PointerId, at: DragPosition) {
        if !self.mounted.load(Ordering::Acquire) {
            return;
        }
        let data = self
            .entered
            .lock()
            .iter()
            .find(|e| e.pointer == pointer)
            .map(|e| Arc::clone(&e.data));
        let Some(data) = data else {
            return;
        };
        self.callbacks().moved(&data, at);
    }

    /// `pointer`'s drag is dropped on this target. Only a current candidate
    /// can be accepted (mirrors the oracle's
    /// `assert(_candidateAvatars.contains(avatar))`); returns whether the drop
    /// was accepted.
    ///
    /// A retired slot accepts nothing: a target that left the tree mid-drag
    /// did not receive the data, and saying otherwise would have the drag
    /// report a completed drop into a widget that no longer exists. The
    /// oracle's `didDrop` returns early on `!mounted` but its caller still
    /// records `wasAccepted = true`; this reports the drop honestly instead.
    ///
    /// The removal happens either way, exactly as in
    /// [`did_leave`](Self::did_leave): "the target did not accept it" must not
    /// also mean "the entry is still there", or a retired slot keeps the
    /// standing — and the drag payload it holds by `Arc` — for as long as
    /// anything holds the slot. Only the callback and the rebuild are gated.
    ///
    /// Flutter parity: `_DragTargetState.didDrop`.
    pub fn did_drop(&self, pointer: PointerId, at: DragPosition) -> bool {
        let dropped = {
            let mut entered = self.entered.lock();
            entered
                .iter()
                .position(|e| e.pointer == pointer && e.accepted)
                .map(|index| entered.remove(index))
        };
        let Some(dropped) = dropped else {
            return false;
        };
        if !self.mounted.load(Ordering::Acquire) {
            return false;
        }
        self.schedule_rebuild();
        self.callbacks().accept(&dropped.data, at);
        true
    }

    /// The erased data of every drag currently over this target, with its
    /// standing — the raw material for
    /// [`DragTargetState::candidate_data`]/[`rejected_data`](DragTargetState::rejected_data).
    fn standings(&self) -> Vec<(ErasedDragData, bool)> {
        self.entered
            .lock()
            .iter()
            .map(|e| (Arc::clone(&e.data), e.accepted))
            .collect()
    }
}

/// Persistent state: the shared [`DragTargetSlot`] this target publishes as
/// its hit-test payload, and from which its builder's candidate/rejected lists
/// are read.
pub struct DragTargetState<T: Clone + Send + Sync + 'static> {
    slot: Arc<DragTargetSlot>,
    /// Ties this state to `DragTarget<T>`: the slot itself is deliberately
    /// non-generic (a drag discovers one without naming `T`), so no field
    /// carries a `T` directly.
    _data: PhantomData<T>,
}

impl<T: Clone + Send + Sync + 'static> std::fmt::Debug for DragTargetState<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DragTargetState")
            .field("candidate_count", &self.candidate_data().len())
            .field("rejected_count", &self.rejected_data().len())
            .finish()
    }
}

impl<T: Clone + Send + Sync + 'static> DragTargetState<T> {
    /// The shared slot this target publishes to hit tests — the object a drag
    /// drives the accept/candidate/reject/leave protocol through.
    #[must_use]
    pub fn slot(&self) -> Arc<DragTargetSlot> {
        Arc::clone(&self.slot)
    }

    /// The candidate data currently over this target, in entry order.
    #[must_use]
    pub fn candidate_data(&self) -> Vec<Option<T>> {
        self.slot
            .standings()
            .iter()
            .filter(|(_, accepted)| *accepted)
            .map(|(data, _)| typed_as(data))
            .collect()
    }

    /// The rejected (`on_will_accept`-vetoed) data currently over this
    /// target, in entry order. See the module docs on why this is typed
    /// (`Vec<T>`) rather than the oracle's `List<dynamic>`.
    #[must_use]
    pub fn rejected_data(&self) -> Vec<T> {
        self.slot
            .standings()
            .iter()
            .filter(|(_, accepted)| !*accepted)
            .filter_map(|(data, _)| typed_as(data))
            .collect()
    }
}

impl<T: Clone + Send + Sync + 'static> StatefulView for DragTarget<T> {
    type State = DragTargetState<T>;

    fn create_state(&self) -> Self::State {
        DragTargetState {
            slot: Arc::new(DragTargetSlot::new(Arc::new(TypedCallbacks::from_view(
                self,
            )))),
            _data: PhantomData,
        }
    }
}

impl<T: Clone + Send + Sync + 'static> ViewState<DragTarget<T>> for DragTargetState<T> {
    /// Publishes the target's rebuild capability into the slot, so a
    /// transition driven from a gesture callback can refresh the builder — a
    /// lifecycle hook, never `build` (port-check trigger #22).
    fn init_state(&mut self, ctx: &dyn BuildContext) {
        self.slot.publish_rebuild(ctx.rebuild_handle());
    }

    /// Retires the slot. A drag that entered this target still holds it by
    /// `Arc`, and would otherwise keep calling into a view whose element is
    /// gone.
    fn dispose(&mut self) {
        self.slot.retire();
    }

    fn build(&self, view: &DragTarget<T>, _ctx: &dyn BuildContext) -> impl IntoView {
        // The current view's closures become the ones a later transition
        // invokes: the slot outlives any one build, so it must not keep a
        // stale rebuild's callbacks.
        self.slot
            .set_callbacks(Arc::new(TypedCallbacks::from_view(view)));

        let candidates = self.candidate_data();
        let rejected = self.rejected_data();

        // The discovery edge: without this the transitions above are real,
        // tested, and unreachable — nothing on a hit path names this target.
        // `Translucent` is the oracle's own default `hitTestBehavior`, and is
        // what makes overlapping targets discoverable: an `Opaque` tag would
        // hide every target beneath it.
        MetaData::shared(Arc::clone(&self.slot) as Arc<dyn Any + Send + Sync>)
            .behavior(HitTestBehavior::Translucent)
            .child((view.builder)(&candidates, &rejected))
    }
}
