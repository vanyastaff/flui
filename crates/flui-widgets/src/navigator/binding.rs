//! [`RouteBinding`] — the owned capability a route uses to drive its own
//! lifecycle: the route-animation seam.
//!
//! `RouteBinding` itself stays private — it can finalize and dispose routes — but
//! the opaque [`RouteBindingSlot`] a route hands the
//! navigator to receive one is exported. See *Correction 2* and its resolution below.
//!
//! # Why routes do not call the navigator directly
//!
//! A route finishing during a flush (e.g. from `did_pop`) needs the navigator to
//! finalize it. Mutating the entry **immediately** while declining to start a
//! *nested* flush is the shape that is needed; a push-completion, by contrast,
//! must never land inside a flush.
//!
//! # Correction 1 to ADR-0020: a direct callback would **deadlock**
//!
//! ADR-0020 proposed a `RouteBinding` exposing `notify_push_completed()` and
//! `finalize()` as direct navigator callbacks. That cannot work.
//! `NavigatorShared::mutate` holds `history.lock()` for the whole flush, and
//! `parking_lot::Mutex` is not reentrant — a route calling back into
//! `RouteHistory` from `did_pop` would hang, not panic.
//!
//! So a binding **enqueues a [`RouteCommand`]** onto a queue guarded by its own
//! mutex, then calls a `wake` closure. `wake` uses `try_lock` on the history: if
//! it succeeds we are outside a flush and the commands are applied and flushed
//! at once; if it fails, a flush is in progress on this thread and *that* flush
//! drains the queue before it returns. The queue is the "is a flush running"
//! check, expressed as ownership rather than as a flag.
//!
//! This preserves both invariants: `RouteHistory` never learns about the
//! navigator (so the route stack stays pure data), and the
//! `BUG: flush_history_updates re-entered` assert stays reachable for a genuinely
//! recursive `flush()` — which is what it was always guarding.
//!
//! **Superseded, `notify_push_completed()` half only (ADR-0064).** A route no
//! longer raises its own `PushCompleted` command: `AnimationController`'s
//! run-starting methods now return the `TickerFuture` the entrance transition
//! ends on, `Route::did_push` hands it out as
//! [`PushCompletion::Animating`](super::route::PushCompletion::Animating),
//! and `NavigatorShared::apply` registers a continuation on it directly —
//! pushing straight onto the same [`RouteCommandQueue`], with no `RouteBinding`
//! and no `wake` closure in between. `finalize()` below is unaffected and
//! still takes the queue-plus-`wake` shape this section describes.
//!
//! # Correction 2 to ADR-0020: `install(&mut self, binding)` cannot be
//!
//! `Route` is public. Threading a `&RouteBinding` through
//! `Route::install` would force `RouteBinding` into the public surface, which
//! was explicitly not authorized. The binding was therefore delivered
//! through `BoundRoute`, a private trait, from a private `push_bound` — which is
//! why `RouteId` is minted up front rather than inside `RouteRecord::erase`.
//!
//! **Resolution.** `PageRoute` and `PopupRoute` are public and must be
//! pushable through the one public `NavigatorHandle::push`, so a private
//! `push_bound` no longer works. `BoundRoute` is gone. In its place
//! [`NavigatorRoute::binding_slot`] returns an optional [`RouteBindingSlot`]: a
//! public, opaque cell with no public accessor. `push` fills it before `install()`.
//! The capability stays private; only the *cell* is public, and a route that does
//! not animate returns `None` and never sees one.
//!
//! [`NavigatorRoute::binding_slot`]: super::overlay_route::NavigatorRoute::binding_slot

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, VecDeque};
use std::fmt;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use flui_animation::{Animation, Curve, Vsync};
use parking_lot::Mutex;

use super::modal_route::ModalHandle;
use super::route::RouteId;
use super::subtree::RouteSubtreeCell;
use crate::OverlayEntry;

/// A lifecycle transition a route asks its navigator to make.
///
/// Applied by [`RouteHistory`](super::history::RouteHistory) either at the head
/// of the next flush, or — when raised *during* a flush — immediately after that
/// flush's walk, which then re-runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RouteCommand {
    /// The entrance transition's `TickerFuture` resolved, complete or
    /// canceled alike: `pushing` → `idle`, then re-flush. Raised
    /// by `NavigatorShared::await_push`'s continuation (ADR-0064), not by a
    /// route through this binding.
    PushCompleted(RouteId),
    /// The route is finished and may be disposed: the entry is finalized, then
    /// a flush runs unless one is already running.
    Finalize(RouteId),
}

/// What one transition route publishes about itself so the route **below** it can
/// drive its `secondary_animation`.
///
/// FLUI's routes are named by [`RouteId`] and live behind
/// `Box<dyn ErasedRoute>` inside a `Mutex`, so a route cannot reach another
/// directly. The registry is the lookup handle.
///
/// `pub` only so `crate::__test_access` can re-export it (ADR-0083 §4); the
/// module is private, so nothing else names it.
#[derive(Clone)]
pub struct TransitionPeer {
    /// The route's **primary** animation, controller-backed.
    pub animation: Arc<dyn Animation<f64>>,
    /// Whether the route *above* can transition from this one.
    pub can_transition_from: bool,
    /// Which family of routes this one coordinates transitions with.
    pub group: TransitionGroup,
    /// Fires when the route is disposed, so the route below can release its
    /// reference to a gone route's animation.
    pub(crate) completed: Arc<CompletedSignal>,
}

impl std::fmt::Debug for TransitionPeer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TransitionPeer")
            .field("can_transition_from", &self.can_transition_from)
            .field("group", &self.group)
            .finish_non_exhaustive()
    }
}

/// The family a route coordinates its transitions with.
///
/// A page route coordinates only with other page routes, while every other
/// transition route coordinates with anything that also leaves the relation
/// open — a symmetric "same family?" relation, which is what this enum
/// encodes. FLUI's routes cannot ask "is the route above a page route" — they
/// name each other by [`RouteId`] and never hold each other's object — so the
/// family travels with the published [`TransitionPeer`].
///
/// A `PopupRoute` pushed over a `PageRoute` therefore drives no secondary
/// animation on the page.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TransitionGroup {
    /// `TransitionRoute`'s defaults: coordinates with anything else that also
    /// leaves both predicates at `true`. `PopupRoute` lives here.
    #[default]
    Default,
    /// `PageRoute`, which coordinates only with other `PageRoute`s.
    Page,
}

/// A one-shot "this route is disposed" owner-local signal with callbacks.
///
/// FLUI's routes are driven synchronously from the flush, so a plain callback
/// list is both sufficient and observable (no future needed). Private: this is
/// the `completed` channel, added **only if** the disposal /
/// train-hopping contract needs it. It does — see `transition_route.rs`.
#[derive(Default)]
pub(crate) struct CompletedSignal {
    done: Cell<bool>,
    listeners: RefCell<Vec<Rc<dyn Fn()>>>,
}

impl CompletedSignal {
    /// Run `callback` when the route completes, or **now** if it already has.
    pub(crate) fn on_completed(&self, callback: Rc<dyn Fn()>) {
        if self.done.get() {
            callback();
            return;
        }
        self.listeners.borrow_mut().push(callback);
    }

    /// Fire once. Later `on_completed` calls run immediately.
    pub(crate) fn complete(&self) {
        if self.done.replace(true) {
            return;
        }
        // Snapshot then fire: a callback may re-enter the route.
        let callbacks = std::mem::take(&mut *self.listeners.borrow_mut());
        for callback in callbacks {
            callback();
        }
    }
}

impl fmt::Debug for CompletedSignal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CompletedSignal")
            .field("done", &self.done.get())
            .finish_non_exhaustive()
    }
}

/// `RouteId -> TransitionPeer`, shared by every binding a navigator mints.
pub(crate) type TransitionRegistry = Arc<Mutex<HashMap<RouteId, TransitionPeer>>>;

/// The clock a route's `AnimationController` registers with.
///
/// **Correction to ADR-0020.** `flui_animation::Vsync` is not a
/// ticker provider but a *registry* a binding drives with `tick_all`. So the
/// seam is not "the navigator is the ticker" but "the navigator owns the `Vsync` its routes register with" —
/// which preserves the property that matters: one clock per navigator, and
/// transitions freeze when the navigator's binding stops ticking.
///
/// `None` when no `VsyncScope` is above the navigator. There is no wall-clock
/// fallback: a route's `AnimationController` is built with
/// [`AnimationController::without_ticker`](flui_animation::AnimationController::without_ticker) —
/// no scheduler, no ticker at all — so with no `Vsync` to register with, the
/// controller simply never advances (the same shape `AnimatedSize` uses).
pub(crate) type RouteVsync = Arc<Mutex<Option<Vsync>>>;

/// `RouteId -> OverlayEntry`, the navigator's map. A route reaches **its own**
/// entry through it; the entries live on the navigator, not the route
/// (`overlay_route.rs`).
pub(crate) type RouteEntries = Arc<Mutex<HashMap<RouteId, OverlayEntry>>>;

/// `RouteId -> RouteSubtreeCell`, the navigator's way to reach a route's page
/// subtree. FLUI's routes live behind `Box<dyn ErasedRoute>` inside the
/// history's mutex, so the route publishes its cell into a registry the navigator
/// owns.
pub(crate) type RouteSubtrees = Arc<Mutex<HashMap<RouteId, RouteSubtreeCell>>>;

/// `RouteId -> ModalHandle`, how the navigator (and the hero controller) set a
/// route's `offstage`. FLUI's routes are unreachable, so a `ModalRoute`
/// publishes its handle here at `install()`.
pub(crate) type RouteModals = Arc<Mutex<HashMap<RouteId, ModalHandle>>>;

/// How a route's exit transition should run, overriding its own default
/// reverse pacing for exactly one pop.
///
/// A back-gesture release animates back with a duration/curve computed from
/// the drag (the fling velocity, or the flat 350ms/
/// `Curves::FastEaseInToSlowEaseOut` "stay" pacing) — never the route's plain
/// reverse. FLUI has no `Route` object a gesture controller can reach to
/// override, so the navigator publishes the override here, keyed by the one
/// route it applies to, and [`RouteBinding::take_pop_pacing`] consumes it
/// exactly once, from that route's own `did_pop`.
/// A gesture-supplied easing curve is a value-only, `Send + Sync` transform
/// (no tree/element access) carried from `back_gesture.rs`'s controller
/// through the pop command to `AnimationController::animate_back_curved` —
/// the same erased-`Animatable`-transform shape ADR-0021 §8 already
/// sanctions for `Hero::create_rect_tween`.
#[derive(Clone)]
pub(crate) struct PopPacing {
    pub(crate) duration: Duration,
    pub(crate) curve: Arc<dyn Curve + Send + Sync>, // see the struct doc — erased easing-curve transform, ADR-0021 §8 shape
}

/// `RouteId -> PopPacing`, a one-shot override the navigator sets immediately
/// before popping a specific route and every path removes on the way out —
/// consumed by a successful pop, and swept by the setter itself if the pop
/// never reaches that route's `did_pop` (a veto, or the route was no longer
/// current). Never a field on the route: a stored field would go stale on a
/// veto or apply to the wrong pop if a *different* route reached `did_pop`
/// first (see `navigator.rs`'s `pop_paced`).
pub(crate) type PopPacingRegistry = Arc<Mutex<HashMap<RouteId, PopPacing>>>;

/// The queue a [`RouteBinding`] writes to and a `RouteHistory` drains.
///
/// Its own mutex, deliberately: it must be lockable while the history's mutex is
/// held by an in-progress flush.
pub(crate) type RouteCommandQueue = Arc<Mutex<VecDeque<RouteCommand>>>;

/// The `RouteId`-keyed maps a navigator owns and every binding shares.
///
/// A bundle rather than five parameters: they are created together in
/// `NavigatorHandle::new`, cloned together into every binding, and each is a
/// `RouteId -> _` map standing in for state a route object would otherwise
/// own directly.
#[derive(Clone)]
pub(crate) struct RouteRegistries {
    /// `RouteId -> TransitionPeer`. A **different** mutex from the history's, so a
    /// route may consult it from inside a flush.
    pub(crate) peers: TransitionRegistry,
    /// `RouteId -> OverlayEntry`.
    pub(crate) entries: RouteEntries,
    /// `RouteId -> RouteSubtreeCell`.
    pub(crate) subtrees: RouteSubtrees,
    /// `RouteId -> ModalHandle`.
    pub(crate) modals: RouteModals,
    /// `RouteId -> PopPacing`, a one-shot override for the next `did_pop`.
    pub(crate) pop_pacing: PopPacingRegistry,
}

/// An owned, `'static` capability, pre-bound to one [`RouteId`].
///
/// A route can only ever drive *itself*: the id is baked in at construction, so
/// no route can finalize another. Cloneable, but owner-local: the wake callback
/// reaches the owning navigator.
///
/// Inert once the navigator is gone: the `wake` closure holds a `Weak`, and a
/// queued command for a route that no longer exists is dropped on drain.
#[derive(Clone)]
pub(crate) struct RouteBinding {
    route: RouteId,
    queue: RouteCommandQueue,
    /// Applies the queue if the history is not currently locked. See *Correction 1*.
    wake: Rc<dyn Fn()>,
    /// The navigator's clock. `Mutex` because `NavigatorState::init_state`
    /// resolves it after the handle (and therefore any seeded binding) exists.
    vsync: RouteVsync,
    /// The navigator's `RouteId`-keyed maps, each behind its own mutex — a
    /// different one from the history's, so a route may consult them from inside a
    /// flush.
    registries: RouteRegistries,
}

impl RouteBinding {
    pub(crate) fn new(
        route: RouteId,
        queue: RouteCommandQueue,
        wake: Rc<dyn Fn()>,
        vsync: RouteVsync,
        registries: RouteRegistries,
    ) -> Self {
        Self {
            route,
            queue,
            wake,
            vsync,
            registries,
        }
    }

    /// This route's overlay entry, or `None` before it is installed.
    ///
    /// Cloned **out** of the map, so the caller never holds the `entries` lock
    /// while touching the overlay.
    fn entry(&self) -> Option<OverlayEntry> {
        self.registries.entries.lock().get(&self.route).cloned()
    }

    /// Set whether this route's overlay entry is opaque.
    pub(crate) fn set_entry_opaque(&self, opaque: bool) {
        if let Some(entry) = self.entry() {
            entry.set_opaque(opaque);
        }
    }

    /// Set whether this route's overlay entry maintains its state while hidden.
    pub(crate) fn set_entry_maintain_state(&self, maintain_state: bool) {
        if let Some(entry) = self.entry() {
            entry.set_maintain_state(maintain_state);
        }
    }

    /// Rebuild **this route's** overlay entry, not the navigator.
    ///
    /// Reached only through `ModalRoute::changed_internal_state`, whose caller is
    /// `ModalHandle::set_offstage` — the `HeroController` seam.
    ///
    /// `dead_code` because that consumer is itself dead until the `Hero` widget
    /// gives it a production caller; see `hero_controller.rs`.
    pub(crate) fn mark_entry_needs_build(&self) {
        if let Some(entry) = self.entry() {
            entry.mark_needs_build();
        }
    }

    /// The navigator's clock, if it has one.
    pub(crate) fn vsync(&self) -> Option<Vsync> {
        self.vsync.lock().clone()
    }

    /// Publish this route's primary animation so the route below can drive its
    /// `secondary_animation` from it.
    pub(crate) fn publish_peer(&self, peer: TransitionPeer) {
        let _prev = self.registries.peers.lock().insert(self.route, peer);
    }

    /// Withdraw it. Called from `dispose`; a peer that outlives its controller
    /// would hand out a disposed animation.
    pub(crate) fn withdraw_peer(&self) {
        let _prev = self.registries.peers.lock().remove(&self.route);
    }

    /// Publish where this route's page subtree *will* live.
    ///
    /// The cell is registered at `install()`, before the page has ever been built,
    /// and resolves to `None` until it mounts. See `subtree.rs`.
    pub(crate) fn publish_subtree(&self, subtree: RouteSubtreeCell) {
        let _prev = self.registries.subtrees.lock().insert(self.route, subtree);
    }

    /// Withdraw it. Called from `dispose`; a registry entry that outlives its route
    /// would let `HeroController` resolve a disposed route's subtree.
    pub(crate) fn withdraw_subtree(&self) {
        let _prev = self.registries.subtrees.lock().remove(&self.route);
    }

    /// Publish this route's `offstage` control, the handle `HeroController`
    /// uses.
    pub(crate) fn publish_modal(&self, modal: ModalHandle) {
        let _prev = self.registries.modals.lock().insert(self.route, modal);
    }

    /// Withdraw it. A disposed route must not be forced offstage.
    pub(crate) fn withdraw_modal(&self) {
        let _prev = self.registries.modals.lock().remove(&self.route);
    }

    /// Consume this route's one-shot [`PopPacing`] override, if the navigator
    /// set one immediately before this pop — see `navigator.rs`'s `pop_paced`.
    /// `None` means "pop normally": the route's own default reverse pacing.
    pub(crate) fn take_pop_pacing(&self) -> Option<PopPacing> {
        self.registries.pop_pacing.lock().remove(&self.route)
    }

    /// The route this binding drives. Every other capability on this type is
    /// already pre-bound to this id; a back-gesture detector needs the id
    /// itself, to bind a drag to the specific route it started on
    /// (`navigator.rs`'s `pop_paced`, `back_gesture.rs`).
    pub(crate) fn route_id(&self) -> RouteId {
        self.route
    }

    /// The peer for `route`, or `None` when it is not a transition route.
    pub(crate) fn peer(&self, route: RouteId) -> Option<TransitionPeer> {
        self.registries.peers.lock().get(&route).cloned()
    }

    /// The route is finished; dispose it.
    pub(crate) fn finalize(&self) {
        self.raise(RouteCommand::Finalize(self.route));
    }

    fn raise(&self, command: RouteCommand) {
        self.queue.lock().push_back(command);
        // Outside a flush this applies and flushes now; inside one it is a no-op
        // and the running flush drains the queue before returning.
        (self.wake)();
    }
}

impl fmt::Debug for RouteBinding {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RouteBinding")
            .field("route", &self.route.get())
            .field("pending", &self.queue.lock().len())
            .finish_non_exhaustive()
    }
}

/// The cell a route hands the navigator so it can receive its own navigator
/// capability before it is pushed.
///
/// **Public but opaque.** A route type stores one, exposes it through
/// [`NavigatorRoute::binding_slot`], and can do nothing else with it: the
/// `RouteBinding` inside is `pub(crate)` and there is no accessor. This is how an
/// animated route gets a navigator capability without that binding — which can
/// finalize and dispose routes — becoming public.
///
/// [`NavigatorRoute::binding_slot`]: super::overlay_route::NavigatorRoute::binding_slot
#[derive(Clone, Default)]
pub struct RouteBindingSlot {
    inner: Arc<Mutex<Option<RouteBinding>>>,
    /// The transition family of the framework route that owns this slot, or
    /// `None` for a slot a third-party route constructed itself. Written only
    /// by the crate's `TransitionRoute`, so a route outside this crate cannot
    /// claim to be a pageless popup: a `Navigator` under a `Router` admits a
    /// route only when this reads `Some(TransitionGroup::Default)`.
    group: Arc<Mutex<Option<TransitionGroup>>>,
}

impl RouteBindingSlot {
    /// An empty slot. A route creates one in its constructor.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Whether the navigator has filled this slot — i.e. the route is pushed.
    #[must_use]
    pub fn is_bound(&self) -> bool {
        self.inner.lock().is_some()
    }

    /// Filled by `NavigatorHandle::push` / `seed_initial`, before `install()`.
    pub(crate) fn fill(&self, binding: RouteBinding) {
        let _prev = self.inner.lock().replace(binding);
    }

    /// The binding, cloned out. `None` for a route that was never pushed, which
    /// is what makes every capability call on an unpushed route inert.
    pub(crate) fn get(&self) -> Option<RouteBinding> {
        self.inner.lock().clone()
    }

    /// Record the owning route's transition family. `TransitionRoute` calls
    /// this at construction and again whenever its family changes.
    pub(crate) fn set_group(&self, group: TransitionGroup) {
        *self.group.lock() = Some(group);
    }

    /// The owning route's transition family; `None` when no framework route
    /// wrote one.
    pub(crate) fn group(&self) -> Option<TransitionGroup> {
        *self.group.lock()
    }
}

/// Whether the route owning `slot` is a pageless popup: a framework transition
/// route in [`TransitionGroup::Default`] (`PopupRoute`). A `Navigator` driven by
/// a `Router` admits these through its facade and refuses every other route —
/// a `PageRoute` (`TransitionGroup::Page`), a slot-less `SimpleRoute`, and a
/// third-party route whose slot no framework route wrote.
pub(crate) fn is_pageless_popup(slot: Option<&RouteBindingSlot>) -> bool {
    slot.and_then(RouteBindingSlot::group) == Some(TransitionGroup::Default)
}

impl fmt::Debug for RouteBindingSlot {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RouteBindingSlot")
            .field("bound", &self.is_bound())
            .finish()
    }
}
