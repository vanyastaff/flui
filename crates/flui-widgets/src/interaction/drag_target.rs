//! [`DragTarget`] — receives typed data when a [`Draggable`](crate::Draggable)
//! is dropped on it.
//!
//! Flutter parity: `widgets/drag_target.dart` (tag `3.44.0`) — `DragTarget`,
//! `_DragTargetState`, `DragTargetDetails`. This is the accept/candidate/
//! reject/leave state machine — the load-bearing, testable core of the
//! `Draggable`/`DragTarget` pair. See [`crate::Draggable`]'s module docs for
//! why it is not yet wired to a *live* hit-test-discovered drag: FLUI has no
//! capability reachable from widget code to ask "what is at this global
//! position" after mount, so `did_enter`/`did_move`/`did_leave`/`did_drop`
//! below are real, tested production methods, driven directly rather than by
//! a live `Draggable` session discovering this target through a pointer
//! move. Tracked in `docs/ROADMAP.md`'s Cross.H section, not just here.
//!
//! # Divergences from the oracle
//!
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
//!   `did_enter` mirrors the same discovery-time filter: a genuinely
//!   type-mismatched payload is never added to either list (see its own
//!   doc), so `did_leave`/`did_move` never need to reconstruct a "was this
//!   ever a real `T`" answer after the fact.

use std::any::Any;
use std::rc::Rc;
use std::sync::Arc;

use flui_interaction::PointerId;
use flui_types::{Offset, geometry::Pixels};
use flui_view::prelude::*;

/// A drag's data, type-erased at the `Draggable`/`DragTarget` boundary so a
/// target can reject a payload whose concrete type does not match `T`
/// (`_DragTargetState.isExpectedDataType`), mirroring Dart's `data is T?`.
pub type ErasedDragData = Arc<dyn Any + Send + Sync>;

/// Details for a [`DragTarget`] callback: the (typed) data and the drop/move
/// position.
///
/// Flutter parity: `DragTargetDetails<T>`.
#[derive(Debug, Clone)]
pub struct DragTargetDetails<T> {
    /// The data carried by the drag.
    pub data: T,
    /// The global position at which the event occurred.
    pub offset: Offset<Pixels>,
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

/// Determines whether a [`DragTarget`] will accept `details`.
pub type DragTargetWillAccept<T> = Rc<dyn Fn(&DragTargetDetails<T>) -> bool>;
/// Fired when an accepted drop lands.
pub type DragTargetAccept<T> = Rc<dyn Fn(DragTargetDetails<T>)>;
/// Fired when a candidate or rejected drag leaves the target.
pub type DragTargetLeave<T> = Rc<dyn Fn(Option<T>)>;
/// Fired on every move while a drag is over the target (candidate or not).
pub type DragTargetMove<T> = Rc<dyn Fn(DragTargetDetails<T>)>;

/// A widget that receives data when a [`Draggable`](crate::Draggable) is
/// dropped on it.
///
/// Flutter parity: `widgets/drag_target.dart` `DragTarget`. See the module
/// docs for the live-discovery deferral.
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

    /// Called when an accepted drag is dropped on the target.
    #[must_use]
    pub fn on_accept(mut self, callback: impl Fn(DragTargetDetails<T>) + 'static) -> Self {
        self.on_accept = Some(Rc::new(callback));
        self
    }

    /// Called when a candidate or rejected drag leaves the target.
    #[must_use]
    pub fn on_leave(mut self, callback: impl Fn(Option<T>) + 'static) -> Self {
        self.on_leave = Some(Rc::new(callback));
        self
    }

    /// Called on every move while a drag (candidate or not) is over the
    /// target.
    #[must_use]
    pub fn on_move(mut self, callback: impl Fn(DragTargetDetails<T>) + 'static) -> Self {
        self.on_move = Some(Rc::new(callback));
        self
    }
}

/// One pointer's standing with a [`DragTarget`]: both variants carry the
/// resolved, downcast `T` — a type mismatch never reaches either variant at
/// all (see [`DragTargetState::did_enter`]'s doc). `Rejected` is exactly the
/// oracle's `_rejectedAvatars`: an `on_will_accept`-vetoed drag whose data
/// already matched `T`, not a foreign-typed one.
enum Standing<T> {
    Candidate(T),
    Rejected(T),
}

impl<T> Standing<T> {
    /// The carried data, whichever standing this is — both variants have one.
    fn data(&self) -> &T {
        match self {
            Standing::Candidate(data) | Standing::Rejected(data) => data,
        }
    }
}

/// Persistent state: the candidate/rejected lists, keyed by the dragging
/// pointer so multiple simultaneous drags are tracked independently
/// (`_DragTargetState._candidateAvatars` / `_rejectedAvatars`).
pub struct DragTargetState<T: Clone + Send + Sync + 'static> {
    entered: Vec<(PointerId, Standing<T>)>,
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
    /// Whether `data`'s concrete type matches `T` (`isExpectedDataType`).
    fn downcast(data: &ErasedDragData) -> Option<T> {
        data.clone().downcast::<T>().ok().map(|arc| (*arc).clone()) // PORT-CHECK-OK-DOWNCAST: mirrors Dart's `data is T?` (`isExpectedDataType`) — a type-mismatched drag is rejected (`did_enter` returns `false`), never panics.
    }

    /// A drag identified by `pointer` enters this target carrying `data` at
    /// `offset`. A `data` whose concrete type does not match `T` is never
    /// tracked at all — no candidate entry, no rejected entry, and returns
    /// `false` without creating anything for `pointer` to leave later. This
    /// mirrors `_getDragTargets`' `isExpectedDataType` filter, which runs
    /// *before* `didEnter` and keeps a type-mismatched avatar out of
    /// `_enteredTargets` entirely — `didEnter` itself, once reached, only
    /// ever decides candidate vs. rejected for already-`T`-typed data via
    /// `on_will_accept`.
    ///
    /// Flutter parity: `_DragTargetState.didEnter`.
    pub fn did_enter(
        &mut self,
        view: &DragTarget<T>,
        pointer: PointerId,
        data: ErasedDragData,
        offset: Offset<Pixels>,
    ) -> bool {
        debug_assert!(
            !self.entered.iter().any(|(id, _)| *id == pointer),
            "BUG: did_enter called twice for the same pointer without an intervening did_leave"
        );

        let Some(typed) = Self::downcast(&data) else {
            // Type mismatch: never becomes an entry, matching the oracle's
            // discovery-time filter — no candidate, no rejected, no future
            // did_leave/did_move/did_drop call for this pointer at all.
            return false;
        };

        let accepted = match &view.on_will_accept {
            None => true,
            Some(callback) => callback(&DragTargetDetails {
                data: typed.clone(),
                offset,
            }),
        };

        let standing = if accepted {
            Standing::Candidate(typed)
        } else {
            Standing::Rejected(typed)
        };
        self.entered.push((pointer, standing));
        accepted
    }

    /// `pointer`'s drag leaves this target — removed from whichever list it
    /// was in, then `on_leave` fires with its data. A no-op for a pointer
    /// that was never tracked (a type mismatch at `did_enter`, or a repeat
    /// call).
    ///
    /// Flutter parity: `_DragTargetState.didLeave`.
    pub fn did_leave(&mut self, view: &DragTarget<T>, pointer: PointerId) {
        let Some(index) = self.entered.iter().position(|(id, _)| *id == pointer) else {
            return;
        };
        let (_, standing) = self.entered.remove(index);
        if let Some(callback) = &view.on_leave {
            let data = match standing {
                Standing::Candidate(data) | Standing::Rejected(data) => data,
            };
            callback(Some(data));
        }
    }

    /// `pointer`'s drag is dropped on this target. Only a current candidate
    /// can be accepted (mirrors the oracle's `assert(_candidateAvatars.contains(avatar))`);
    /// returns whether the drop was accepted.
    ///
    /// Flutter parity: `_DragTargetState.didDrop`.
    pub fn did_drop(
        &mut self,
        view: &DragTarget<T>,
        pointer: PointerId,
        offset: Offset<Pixels>,
    ) -> bool {
        let Some(index) = self.entered.iter().position(|(id, _)| *id == pointer) else {
            return false;
        };
        let Standing::Candidate(_) = &self.entered[index].1 else {
            return false;
        };
        let (_, Standing::Candidate(data)) = self.entered.remove(index) else {
            unreachable!("BUG: checked Candidate above, remove must yield the same variant");
        };
        if let Some(callback) = &view.on_accept {
            callback(DragTargetDetails { data, offset });
        }
        true
    }

    /// `pointer`'s drag moves while over this target — fires `on_move` for
    /// **either** standing (candidate or rejected), matching the oracle's
    /// `didMove`, whose only gate is `avatar.data == null` (a genuinely null
    /// payload, not rejection status: a vetoed-but-typed avatar still sits in
    /// `_enteredTargets` and receives moves). A no-op only for an untracked
    /// pointer (never entered, or a type mismatch at `did_enter`).
    ///
    /// Flutter parity: `_DragTargetState.didMove`.
    pub fn did_move(&self, view: &DragTarget<T>, pointer: PointerId, offset: Offset<Pixels>) {
        let Some((_, standing)) = self.entered.iter().find(|(id, _)| *id == pointer) else {
            return;
        };
        if let Some(callback) = &view.on_move {
            callback(DragTargetDetails {
                data: standing.data().clone(),
                offset,
            });
        }
    }

    /// The candidate data currently over this target, in entry order.
    #[must_use]
    pub fn candidate_data(&self) -> Vec<Option<T>> {
        self.entered
            .iter()
            .filter_map(|(_, standing)| match standing {
                Standing::Candidate(data) => Some(Some(data.clone())),
                Standing::Rejected(_) => None,
            })
            .collect()
    }

    /// The rejected (`on_will_accept`-vetoed) data currently over this
    /// target, in entry order. See the module docs on why this is typed
    /// (`Vec<T>`) rather than the oracle's `List<dynamic>`.
    #[must_use]
    pub fn rejected_data(&self) -> Vec<T> {
        self.entered
            .iter()
            .filter_map(|(_, standing)| match standing {
                Standing::Rejected(data) => Some(data.clone()),
                Standing::Candidate(_) => None,
            })
            .collect()
    }
}

impl<T: Clone + Send + Sync + 'static> StatefulView for DragTarget<T> {
    type State = DragTargetState<T>;

    fn create_state(&self) -> Self::State {
        DragTargetState {
            entered: Vec::new(),
        }
    }
}

impl<T: Clone + Send + Sync + 'static> ViewState<DragTarget<T>> for DragTargetState<T> {
    fn build(&self, view: &DragTarget<T>, _ctx: &dyn BuildContext) -> impl IntoView {
        let candidates = self.candidate_data();
        let rejected = self.rejected_data();
        (view.builder)(&candidates, &rejected)
    }
}
