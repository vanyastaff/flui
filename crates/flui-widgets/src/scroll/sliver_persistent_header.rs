//! [`SliverPersistentHeader`] — a sliver whose child is rebuilt as the header
//! collapses, pinnable and floatable.

use std::rc::Rc;
use std::sync::Arc;

use flui_animation::{AnimationController, Vsync, VsyncRegistration};
use flui_foundation::ListenerId;
use flui_objects::SnapCommand;
use flui_rendering::view::{ScrollDirection, ScrollPosition};
use flui_view::BuildContextExt as _;
use flui_view::element::{
    FloatingPersistentHeaderView, FloatingPinnedPersistentHeaderView, PinnedPersistentHeaderView,
    ScrollingPersistentHeaderView, SharedHeaderDelegate, SliverPersistentHeaderDelegate,
};
use flui_view::prelude::{StatefulView, StatelessView, ViewState};
use flui_view::{BuildContext, IntoView, RebuildHandle, ViewExt};

use crate::VsyncScope;
use crate::scroll::scroll_position_scope::ScrollPositionScope;

/// A sliver that keeps a header of delegate-controlled extent at the leading
/// edge, rebuilding its content as the header collapses from
/// [`max_extent`](SliverPersistentHeaderDelegate::max_extent) toward
/// [`min_extent`](SliverPersistentHeaderDelegate::min_extent).
///
/// The [`SliverPersistentHeaderDelegate`]'s `build` receives the header's real
/// published collapse state — `shrink_offset` and `overlaps_content` — every
/// time it changes, in the same frame it changed (the build-during-layout
/// fixpoint `LayoutBuilder` also rides). This is the foundation collapsing app
/// bars sit on.
///
/// `pinned` keeps the collapsed header on screen; `floating` re-reveals it on
/// any scroll toward the start. The four combinations map to four distinct
/// render objects, so flipping a flag replaces the element rather than
/// mutating it — exactly Flutter's shape, where each combination is its own
/// internal widget.
///
/// # Stretch is on the delegate; snap is deferred, deliberately
///
/// Over-scroll stretch is configured per delegate —
/// [`SliverPersistentHeaderDelegate::stretch_configuration`] — matching
/// Flutter's placement, so this widget carries no stretch knob of its own.
/// Snap (`FloatingHeaderSnapConfiguration`) remains unreachable: starting a
/// snap needs a scroll-end trigger from the enclosing scrollable, which is
/// the `SliverAppBar` integration seam. Until that lands, floating headers
/// scroll freely and never snap.
///
/// Flutter parity: `widgets/sliver_persistent_header.dart`
/// `SliverPersistentHeader`.
#[derive(Clone, StatelessView)]
pub struct SliverPersistentHeader {
    delegate: Rc<dyn SliverPersistentHeaderDelegate>, // PORT-CHECK-OK-DYN: carries flui-view's SharedHeaderDelegate erasure (justified at its declaration) through the facade
    pinned: bool,
    floating: bool,
}

impl SliverPersistentHeader {
    /// A scrolling (neither pinned nor floating) header over `delegate`.
    pub fn new(delegate: impl SliverPersistentHeaderDelegate + 'static) -> Self {
        Self {
            delegate: Rc::new(delegate),
            pinned: false,
            floating: false,
        }
    }

    /// Keep the collapsed header visible at the leading edge instead of
    /// letting it scroll away.
    #[must_use]
    pub fn pinned(mut self, pinned: bool) -> Self {
        self.pinned = pinned;
        self
    }

    /// Re-reveal the header as soon as the user scrolls toward the start,
    /// regardless of how far away it was.
    #[must_use]
    pub fn floating(mut self, floating: bool) -> Self {
        self.floating = floating;
        self
    }
}

impl std::fmt::Debug for SliverPersistentHeader {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SliverPersistentHeader")
            .field("pinned", &self.pinned)
            .field("floating", &self.floating)
            .field("min_extent", &self.delegate.min_extent())
            .field("max_extent", &self.delegate.max_extent())
            .finish_non_exhaustive()
    }
}

impl StatelessView for SliverPersistentHeader {
    fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
        let delegate = Rc::clone(&self.delegate);
        // Four distinct view TYPES, not one view with flags: the reconciler
        // answers a flag flip by replacing the element, which is the only
        // correct answer when each variant owns a different render object.
        match (self.pinned, self.floating) {
            (false, false) => ScrollingPersistentHeaderView::new(delegate).boxed(),
            (true, false) => PinnedPersistentHeaderView::new(delegate).boxed(),
            // Floating variants route through the host, which owns the
            // snap machinery (controller + scroll-activity trigger). With a
            // snap-less delegate the host is a transparent pass-through.
            (false, true) => FloatingHeaderHost {
                delegate,
                pinned: false,
            }
            .boxed(),
            (true, true) => FloatingHeaderHost {
                delegate,
                pinned: true,
            }
            .boxed(),
        }
    }
}

// ---------------------------------------------------------------------------
// Floating host: the snap trigger
// ---------------------------------------------------------------------------

/// Hosts a floating header's snap machinery: the animation controller (built
/// here, where the ambient [`VsyncScope`] lives) and the scroll-activity
/// trigger.
///
/// # How a snap fires
///
/// The enclosing [`Scrollable`](super::Scrollable) publishes its shared
/// [`ScrollPosition`] via [`ScrollPositionScope`], and marks scroll activity
/// on it at every gesture edge. This host subscribes an activity listener in
/// `init_state`; the listener remembers the last non-idle user direction
/// and, when scrolling ENDS, stamps a fresh epoch onto a
/// [`SnapCommand`] and schedules a rebuild through the ADR-0018
/// [`RebuildHandle`]. The rebuilt view carries the command to
/// `update_render_object`, which the floating render object applies
/// epoch-idempotently — the mutation rides the canonical update path, never
/// a listener reaching into the render tree.
///
/// The listener only ever runs on the UI owner thread (every activity
/// transition originates from gesture callbacks or the fling controller's
/// vsync-driven status listener), but it deliberately touches nothing except
/// the shared pending-command slot and the rebuild handle.
#[derive(Clone, StatefulView)]
struct FloatingHeaderHost {
    delegate: SharedHeaderDelegate,
    pinned: bool,
}

impl std::fmt::Debug for FloatingHeaderHost {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FloatingHeaderHost")
            .field("pinned", &self.pinned)
            .finish_non_exhaustive()
    }
}

/// The listener↔build shared slot. Every touch is owner-thread today (see
/// the host's own doc), but the activity-listener type is structurally
/// `Send + Sync`, so the slot carries a real lock rather than a promise.
#[derive(Default)]
struct SnapTriggerSlot {
    /// The last non-idle direction observed while scrolling — what the
    /// scroll that just ended was doing. Captured here because ending the
    /// scroll resets the position's own direction to `Idle` before
    /// listeners run.
    last_direction: Option<ScrollDirection>,
    /// Monotone stamp for [`SnapCommand`]s issued by this host.
    epoch: u64,
    /// The command the next build will carry, `None` until the first
    /// scroll ends.
    pending: Option<SnapCommand>,
}

struct FloatingHeaderHostState {
    snap_controller: Option<AnimationController>,
    vsync: Option<Vsync>,
    vsync_registration: Option<VsyncRegistration>,
    position: Option<ScrollPosition>,
    activity_listener: Option<ListenerId>,
    slot: Arc<parking_lot::Mutex<SnapTriggerSlot>>,
}

impl std::fmt::Debug for FloatingHeaderHostState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FloatingHeaderHostState")
            .field("has_controller", &self.snap_controller.is_some())
            .field("subscribed", &self.activity_listener.is_some())
            .finish_non_exhaustive()
    }
}

impl StatefulView for FloatingHeaderHost {
    type State = FloatingHeaderHostState;

    fn create_state(&self) -> Self::State {
        FloatingHeaderHostState {
            snap_controller: None,
            vsync: None,
            vsync_registration: None,
            position: None,
            activity_listener: None,
            slot: Arc::new(parking_lot::Mutex::new(SnapTriggerSlot::default())),
        }
    }
}

impl ViewState<FloatingHeaderHost> for FloatingHeaderHostState {
    fn init_state(&mut self, ctx: &dyn BuildContext) {
        // Everything below is lifecycle-only capability acquisition
        // (ADR-0018 / ADR-0021 discipline): the handles are taken HERE and
        // used later, from the listener.
        // The controller: built where the vsync lives. Duration is a
        // placeholder — `maybe_start_snap_animation` re-applies the snap
        // configuration's own duration on every start.
        let controller = AnimationController::without_ticker(std::time::Duration::from_millis(200));
        if let Some(vsync) = ctx.get::<VsyncScope, _>(|scope| scope.vsync().clone()) {
            let registration = vsync.register(controller.clone());
            self.vsync = Some(vsync);
            self.vsync_registration = Some(registration);
        }
        self.snap_controller = Some(controller);

        // The trigger: subscribe to the enclosing scrollable's activity.
        // No scope (a header outside any Scrollable, or in a purely
        // programmatic scroll view) simply means no snap trigger — the
        // header still floats; it just never settles by animation.
        if let Some(position) = ctx.get::<ScrollPositionScope, _>(|scope| scope.position().clone())
        {
            let rebuild: RebuildHandle = ctx.rebuild_handle();
            let slot = Arc::clone(&self.slot);
            let listener_position = position.clone();
            let listener = OwnerThreadSnapListener {
                slot,
                position: listener_position,
                rebuild,
            };
            let id = position.add_activity_listener(std::sync::Arc::new(move || {
                listener.on_activity();
            }));
            self.position = Some(position);
            self.activity_listener = Some(id);
        }
    }

    fn build(&self, view: &FloatingHeaderHost, _ctx: &dyn BuildContext) -> impl IntoView {
        let command = self.slot.lock().pending;
        let controller = self.snap_controller.clone();
        if view.pinned {
            FloatingPinnedPersistentHeaderView::new(Rc::clone(&view.delegate))
                .with_snap_controller(controller)
                .with_snap_command(command)
                .boxed()
        } else {
            FloatingPersistentHeaderView::new(Rc::clone(&view.delegate))
                .with_snap_controller(controller)
                .with_snap_command(command)
                .boxed()
        }
    }

    fn dispose(&mut self) {
        if let (Some(position), Some(id)) = (self.position.take(), self.activity_listener.take()) {
            position.remove_activity_listener(id);
        }
        if let (Some(vsync), Some(registration)) =
            (self.vsync.take(), self.vsync_registration.take())
        {
            vsync.unregister(registration);
        }
    }
}

/// The activity listener's owner-thread half, named so the safety argument
/// above has an anchor.
struct OwnerThreadSnapListener {
    slot: Arc<parking_lot::Mutex<SnapTriggerSlot>>,
    position: ScrollPosition,
    rebuild: RebuildHandle,
}

impl OwnerThreadSnapListener {
    fn on_activity(&self) {
        let is_scrolling = self.position.is_scrolling();
        let direction = self.position.user_scroll_direction();
        let mut slot = self.slot.lock();
        if is_scrolling {
            if direction != ScrollDirection::Idle {
                slot.last_direction = Some(direction);
            }
            return;
        }
        // Scrolling just ended: the position's own direction is already
        // reset, so the captured one is the scroll that ended. No captured
        // direction (a programmatic jump) ⇒ no snap, matching Flutter,
        // where snapping keys on user gestures only.
        let Some(direction) = slot.last_direction.take() else {
            return;
        };
        slot.epoch += 1;
        slot.pending = Some(SnapCommand {
            epoch: slot.epoch,
            direction,
        });
        drop(slot);
        self.rebuild
            .schedule(flui_view::RebuildReason::AnimationTick);
    }
}
