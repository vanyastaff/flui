//! [`DragTarget`] — receives typed data when a [`Draggable`](crate::Draggable)
//! is dropped on it.
//!
//! Flutter parity: `widgets/drag_target.dart` (tag `3.44.0`) — `DragTarget`,
//! `_DragTargetState`, `DragTargetDetails`. This is the accept/candidate/
//! reject/leave state machine, and it is now **live**: the target registers
//! its [`DragTargetSlot`] in the owner's interaction lane and tags its render
//! node with the lane's ticket, and a dragging [`Draggable`](crate::Draggable)
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
//!   live on the *view*, which the state does not own. So the target keeps an
//!   owner-local `Rc<DragTargetSlot>` that carries both the entered list and
//!   the callbacks, registers it in the owner's interaction lane, and
//!   publishes only the lane's `Send + Sync` ticket as hit-test metadata; a
//!   drag resolves the ticket back to the slot on the owner thread. Each build
//!   refreshes the callbacks into the slot, and [`DragTargetState`] reads its
//!   candidate/rejected lists back out of it. Recorded in
//!   `crates/flui-widgets/ARCHITECTURE.md` (`## Mapping decisions`).
//! - **Callbacks are owner-local and receive an `EventCx`.** `on_accept`,
//!   `on_leave` and `on_move` run inside a write the target's `WriterSource`
//!   opens (ADR-0086), synchronously inside the drag's own dispatch, so a
//!   drop lands before the draggable's `on_drag_end`, as in the oracle's
//!   `finishDrag`. `on_will_accept` is a query and takes no `EventCx`.
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
use std::cell::{Cell, RefCell};
use std::marker::PhantomData;
use std::rc::Rc;
use std::sync::Arc;

use flui_foundation::geometry::Offset;
use flui_interaction::{LocalPayloadTarget, PointerId};
use flui_objects::RenderMetaData;
use flui_rendering::hit_testing::HitTestBehavior;
use flui_rendering::protocol::BoxProtocol;
use flui_view::prelude::*;
use flui_view::{
    Child, EventCx, EventOutcome, RebuildHandle, RenderObjectContext, RenderView, WriterSource,
    impl_render_view,
};

use crate::support::value_callback;

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
    pub global: Offset<f64>,
    /// The same position in the target's own local coordinate space.
    pub local: Offset<f64>,
}

impl DragPosition {
    /// A position whose local space *is* the global space — for a target with
    /// no transform between it and the root, and for direct callers driving
    /// the protocol without a hit-test path.
    #[must_use]
    pub fn global_only(global: Offset<f64>) -> Self {
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
    pub offset: Offset<f64>,
    /// The same position in this target's own local coordinate space.
    pub local_offset: Offset<f64>,
}

/// Builds a [`DragTarget`]'s contents from its current candidate/rejected
/// state.
///
/// Flutter parity: `DragTargetBuilder<T>`, minus the `BuildContext` parameter
/// (the target's own `build` already has one available if the builder needs
/// ambient lookups — the candidate/rejected data is what changes per drag),
/// and a typed `&[T]` rejected list rather than `List<dynamic>` — see the
/// module docs on why that is a faithful narrowing, not a divergence.
pub type DragTargetBuilder<T> = Rc<dyn Fn(&[Option<T>], &[T]) -> BoxedView>;

/// Determines whether a [`DragTarget`] will accept `details`. A query, so it
/// receives no `EventCx` (ADR-0086 §6).
pub type DragTargetWillAccept<T> = Rc<dyn Fn(&DragTargetDetails<T>) -> bool>;
/// Fired when an accepted drop lands.
pub type DragTargetAccept<T> = Rc<dyn Fn(&mut EventCx<'_>, DragTargetDetails<T>)>;
/// Fired when a candidate or rejected drag leaves the target.
pub type DragTargetLeave<T> = Rc<dyn Fn(&mut EventCx<'_>, Option<T>)>;
/// Fired on every move while a drag is over the target (candidate or not).
pub type DragTargetMove<T> = Rc<dyn Fn(&mut EventCx<'_>, DragTargetDetails<T>)>;

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
        callback: impl Fn(&DragTargetDetails<T>) -> bool + 'static,
    ) -> Self {
        self.on_will_accept = Some(Rc::new(callback));
        self
    }

    /// Called when an accepted drag is dropped on the target, before the
    /// draggable's own `on_drag_end`, with the drop's `&mut EventCx<'_>`.
    #[must_use]
    pub fn on_accept<F, R>(mut self, callback: F) -> Self
    where
        F: Fn(&mut EventCx<'_>, DragTargetDetails<T>) -> R + 'static,
        R: EventOutcome,
    {
        self.on_accept = Some(value_callback(callback));
        self
    }

    /// Called when a candidate or rejected drag leaves the target.
    #[must_use]
    pub fn on_leave<F, R>(mut self, callback: F) -> Self
    where
        F: Fn(&mut EventCx<'_>, Option<T>) -> R + 'static,
        R: EventOutcome,
    {
        self.on_leave = Some(value_callback(callback));
        self
    }

    /// Called on every move while a drag (candidate or not) is over the
    /// target.
    #[must_use]
    pub fn on_move<F, R>(mut self, callback: F) -> Self
    where
        F: Fn(&mut EventCx<'_>, DragTargetDetails<T>) -> R + 'static,
        R: EventOutcome,
    {
        self.on_move = Some(value_callback(callback));
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
    payload.downcast::<T>().ok().map(|typed| (*typed).clone()) // reverses this boundary's own erasure; see this function's doc.
}

/// One `DragTarget<T>`'s callbacks as a *drag* sees them.
///
/// A drag carries one erased payload and walks a hit path of targets whose
/// `T`s it cannot name, so the per-target `T` has to be discharged on the
/// target's side of the boundary. Each method takes the erased data and
/// downcasts once, here, where `T` is still in scope. The `has_*` answers let
/// the slot skip opening a write for a callback that is not set.
trait TargetCallbacks {
    /// Whether `data`'s concrete type is this target's `T`
    /// (`_DragTargetState.isExpectedDataType`).
    fn accepts_data_type(&self, data: &ErasedDragData) -> bool;
    /// The `on_will_accept` veto. `true` (candidate) when unset.
    fn will_accept(&self, data: &ErasedDragData, at: DragPosition) -> bool;
    fn has_leave(&self) -> bool;
    fn has_move(&self) -> bool;
    fn has_accept(&self) -> bool;
    fn leave(&self, cx: &mut EventCx<'_>, data: &ErasedDragData);
    fn moved(&self, cx: &mut EventCx<'_>, data: &ErasedDragData, at: DragPosition);
    fn accept(&self, cx: &mut EventCx<'_>, data: &ErasedDragData, at: DragPosition);
}

/// One target's callbacks, shared between its element and every drag that has
/// discovered it.
type SharedTargetCallbacks = Rc<dyn TargetCallbacks>; // a drag drives targets whose `T` it cannot name — see `TargetCallbacks`.

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

    fn has_leave(&self) -> bool {
        self.on_leave.is_some()
    }

    fn has_move(&self) -> bool {
        self.on_move.is_some()
    }

    fn has_accept(&self) -> bool {
        self.on_accept.is_some()
    }

    fn leave(&self, cx: &mut EventCx<'_>, data: &ErasedDragData) {
        if let Some(callback) = &self.on_leave {
            callback(cx, typed_as(data));
        }
    }

    fn moved(&self, cx: &mut EventCx<'_>, data: &ErasedDragData, at: DragPosition) {
        if let Some(callback) = &self.on_move
            && let Some(details) = Self::details(data, at)
        {
            callback(cx, details);
        }
    }

    fn accept(&self, cx: &mut EventCx<'_>, data: &ErasedDragData, at: DragPosition) {
        if let Some(callback) = &self.on_accept
            && let Some(details) = Self::details(data, at)
        {
            callback(cx, details);
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

/// The owner-local handle a [`DragTarget`] registers for hit tests to find,
/// and the object a drag drives its transitions through.
///
/// It exists because the two halves of the oracle's `_DragTargetState` cannot
/// travel together in FLUI: a hit-test payload is `Arc<dyn Any + Send + Sync>`
/// and the callbacks live on the view. The slot owns the entered list, holds
/// the current build's callbacks, and knows how to schedule the target's
/// rebuild and open its callbacks' writes — so a drag that resolves one from a
/// hit path can run the whole protocol against it without ever naming the
/// target's `T` or touching the element tree.
///
/// The slot lives in the owner's interaction lane, and the hit-test payload is
/// only the lane's `Send + Sync` ticket for it, resolved back on the owner
/// thread. Shared by `Rc`, and deliberately outliving its element: a drag that
/// has entered a target keeps the slot alive, so a target unmounting mid-drag
/// leaves the drag with a valid — if retired — handle instead of a dangling
/// one. A retired slot answers every transition as a no-op, which is the
/// oracle's `if (!mounted) return;` guard in a form that cannot be forgotten at
/// one call site.
///
/// Every borrow of the slot's cells ends before a user callback runs, so a
/// callback that rebuilds or unmounts its own target cannot collide with one.
pub struct DragTargetSlot {
    /// The current build's callbacks. Cloned out before every invocation.
    callbacks: RefCell<SharedTargetCallbacks>,
    /// Every drag currently over this target, keyed by pointer so several
    /// simultaneous drags stay independent (`_candidateAvatars` /
    /// `_rejectedAvatars`, which the oracle keys by avatar identity).
    entered: RefCell<Vec<EnteredDrag>>,
    /// The target element's rebuild capability, published by
    /// `DragTargetState::init_state` — never from `build`. Stands in for the
    /// oracle's `setState`.
    rebuild: RefCell<Option<RebuildHandle>>,
    /// Opens the `EventCx` each transition callback runs in (ADR-0086),
    /// published by `DragTargetState::init_state` alongside `rebuild`.
    writer: RefCell<Option<WriterSource>>,
    /// `false` once the target's element is disposed.
    mounted: Cell<bool>,
}

impl std::fmt::Debug for DragTargetSlot {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DragTargetSlot")
            .field("entered", &self.entered.borrow().len())
            .field("mounted", &self.mounted.get())
            .finish_non_exhaustive()
    }
}

impl DragTargetSlot {
    fn new(callbacks: SharedTargetCallbacks) -> Self {
        Self {
            callbacks: RefCell::new(callbacks),
            entered: RefCell::new(Vec::new()),
            rebuild: RefCell::new(None),
            writer: RefCell::new(None),
            mounted: Cell::new(true),
        }
    }

    fn set_callbacks(&self, callbacks: SharedTargetCallbacks) {
        // Per-build refresh; the previous set drops after the borrow ends.
        let _prev = self.callbacks.replace(callbacks);
    }

    fn publish_lifecycle(&self, rebuild: RebuildHandle, writer: WriterSource) {
        let _prev = self.rebuild.replace(Some(rebuild));
        let _prev = self.writer.replace(Some(writer));
    }

    /// The target's element has gone. Every later transition is a no-op.
    fn retire(&self) {
        self.mounted.set(false);
    }

    /// Clone the callbacks out of their cell, so user code never runs while
    /// the slot holds a borrow.
    fn callbacks(&self) -> SharedTargetCallbacks {
        Rc::clone(&self.callbacks.borrow())
    }

    /// The oracle's `setState`: the candidate/rejected lists the builder reads
    /// just changed.
    fn schedule_rebuild(&self) {
        // Cloned out and the borrow dropped before calling into the framework.
        let handle = self.rebuild.borrow().clone();
        if let Some(handle) = handle {
            handle.schedule(flui_view::RebuildReason::StateChange);
        }
    }

    /// Run `invoke` inside a write this target's writer opens.
    ///
    /// A slot whose state never reached `init_state` has no writer; its
    /// callback is dropped with a warning rather than run without an
    /// `EventCx` or panicked over.
    fn write(&self, invoke: impl FnOnce(&mut EventCx<'_>)) {
        let writer = self.writer.borrow().clone();
        if let Some(writer) = writer {
            writer.write(invoke);
        } else {
            tracing::warn!(
                "a drag-target callback was dropped: its target has no writer yet \
                 (the state has not run init_state)"
            );
        }
    }

    /// Whether this target's `T` is `data`'s concrete type — the oracle's
    /// `isExpectedDataType`, which `_getDragTargets` applies to filter the hit
    /// path *before* any transition runs.
    #[must_use]
    pub fn accepts_data_type(&self, data: &ErasedDragData) -> bool {
        self.mounted.get() && self.callbacks().accepts_data_type(data)
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
        if !self.mounted.get() {
            return false;
        }
        debug_assert!(
            !self.entered.borrow().iter().any(|e| e.pointer == pointer),
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
        self.entered.borrow_mut().push(EnteredDrag {
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
            let mut entered = self.entered.borrow_mut();
            entered
                .iter()
                .position(|e| e.pointer == pointer)
                .map(|index| entered.remove(index))
        };
        let Some(removed) = removed else {
            return;
        };
        if !self.mounted.get() {
            return;
        }
        self.schedule_rebuild();
        let callbacks = self.callbacks();
        if callbacks.has_leave() {
            self.write(|cx| callbacks.leave(cx, &removed.data));
        }
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
        if !self.mounted.get() {
            return;
        }
        let data = self
            .entered
            .borrow()
            .iter()
            .find(|e| e.pointer == pointer)
            .map(|e| Arc::clone(&e.data));
        let Some(data) = data else {
            return;
        };
        let callbacks = self.callbacks();
        if callbacks.has_move() {
            self.write(|cx| callbacks.moved(cx, &data, at));
        }
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
            let mut entered = self.entered.borrow_mut();
            entered
                .iter()
                .position(|e| e.pointer == pointer && e.accepted)
                .map(|index| entered.remove(index))
        };
        let Some(dropped) = dropped else {
            return false;
        };
        if !self.mounted.get() {
            return false;
        }
        self.schedule_rebuild();
        let callbacks = self.callbacks();
        if callbacks.has_accept() {
            self.write(|cx| callbacks.accept(cx, &dropped.data, at));
        }
        true
    }

    /// The erased data of every drag currently over this target, with its
    /// standing — the raw material for
    /// [`DragTargetState::candidate_data`]/[`rejected_data`](DragTargetState::rejected_data).
    fn standings(&self) -> Vec<(ErasedDragData, bool)> {
        self.entered
            .borrow()
            .iter()
            .map(|e| (Arc::clone(&e.data), e.accepted))
            .collect()
    }
}

/// Persistent state: the owner-local [`DragTargetSlot`] this target registers
/// for hit tests, and from which its builder's candidate/rejected lists are
/// read.
pub struct DragTargetState<T: Clone + Send + Sync + 'static> {
    slot: Rc<DragTargetSlot>,
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
    /// The slot this target registers for hit tests — the object a drag
    /// drives the accept/candidate/reject/leave protocol through.
    #[must_use]
    pub fn slot(&self) -> Rc<DragTargetSlot> {
        Rc::clone(&self.slot)
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
            slot: Rc::new(DragTargetSlot::new(Rc::new(TypedCallbacks::from_view(
                self,
            )))),
            _data: PhantomData,
        }
    }
}

impl<T: Clone + Send + Sync + 'static> ViewState<DragTarget<T>> for DragTargetState<T> {
    /// Publishes the target's rebuild capability and writer into the slot, so
    /// a transition driven from a gesture callback can refresh the builder and
    /// open its callbacks' writes — a lifecycle hook, never `build`.
    fn init_state(&mut self, ctx: &dyn LifecycleContext) {
        self.slot
            .publish_lifecycle(ctx.rebuild_handle(), ctx.writer_source());
    }

    /// Retires the slot. A drag that entered this target still holds it by
    /// `Rc`, and would otherwise keep calling into a view whose element is
    /// gone.
    fn dispose(&mut self) {
        self.slot.retire();
    }

    fn build(&self, view: &DragTarget<T>, _ctx: &dyn BuildContext) -> impl IntoView {
        // The current view's closures become the ones a later transition
        // invokes: the slot outlives any one build, so it must not keep a
        // stale rebuild's callbacks.
        self.slot
            .set_callbacks(Rc::new(TypedCallbacks::from_view(view)));

        let candidates = self.candidate_data();
        let rejected = self.rejected_data();

        // The discovery edge: without this the transitions above are real,
        // tested, and unreachable — nothing on a hit path names this target.
        DragTargetAnchor {
            slot: Rc::clone(&self.slot),
            child: Child::some((view.builder)(&candidates, &rejected)),
        }
    }
}

/// Registers a target's slot in the owner lane and tags its child's position
/// in the render tree with the lane's ticket, so a drag's hit test finds it.
///
/// `Translucent` is the oracle's own default `hitTestBehavior`, and is what
/// makes overlapping targets discoverable: an `Opaque` tag would hide every
/// target beneath it.
#[derive(Clone)]
struct DragTargetAnchor {
    slot: Rc<DragTargetSlot>,
    child: Child,
}

impl DragTargetAnchor {
    /// Register the slot and publish its ticket. A context with no owner lane
    /// (a detached mount) leaves the target undiscoverable until an update
    /// under a lane registers it.
    fn publish(&self, ctx: &RenderObjectContext<'_>, render_object: &mut RenderMetaData) {
        let payload: Rc<dyn Any> = Rc::clone(&self.slot) as Rc<dyn Any>;
        match ctx.register_local_payload(payload) {
            Ok(target) => {
                render_object.set_shared_metadata(Some(Arc::new(target)));
            }
            Err(error) => tracing::debug!(
                ?error,
                "drag target mounted without an owner lane; drags cannot discover it yet"
            ),
        }
    }
}

impl RenderView for DragTargetAnchor {
    type Protocol = BoxProtocol;
    type RenderObject = RenderMetaData;

    fn create_render_object(&self, ctx: &RenderObjectContext<'_>) -> Self::RenderObject {
        let mut render_object = RenderMetaData::new();
        render_object.set_behavior(HitTestBehavior::Translucent);
        self.publish(ctx, &mut render_object);
        render_object
    }

    fn update_render_object(
        &self,
        ctx: &RenderObjectContext<'_>,
        render_object: &mut Self::RenderObject,
    ) -> flui_rendering::RenderUpdateImpact {
        // The slot is the state's own, the same across rebuilds, so an
        // existing ticket still resolves to it. The ticket is read only by a
        // hit test, which reads live state, so no update impact either way.
        if render_object.metadata_as::<LocalPayloadTarget>().is_none() {
            self.publish(ctx, render_object);
        }
        flui_rendering::RenderUpdateImpact::NONE
    }

    fn did_unmount_render_object(
        &self,
        ctx: &RenderObjectContext<'_>,
        render_object: &mut Self::RenderObject,
    ) {
        if let Some(target) = render_object.metadata_as::<LocalPayloadTarget>().copied()
            && let Err(error) = ctx.unregister_local_payload(target)
        {
            tracing::debug!(?error, "drag target slot was already unregistered");
        }
        render_object.clear_metadata();
    }

    flui_view::single_child_view_children!();
}

impl_render_view!(DragTargetAnchor);
