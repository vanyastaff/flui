//! [`SliverPersistentHeader`] — a sliver whose child is rebuilt as the header
//! collapses, pinnable and floatable.

use std::cell::Cell;
use std::rc::Rc;

use flui_animation::{AnimationController, DrivenController};
use flui_foundation::ListenerId;
use flui_objects::{SnapAction, SnapCommand};
use flui_rendering::view::{ScrollDirection, ScrollPosition};
use flui_view::BuildContextExt as _;
use flui_view::element::{
    FloatingPersistentHeaderView, FloatingPinnedPersistentHeaderView, PinnedPersistentHeaderView,
    ScrollingPersistentHeaderView, SharedHeaderDelegate, SliverPersistentHeaderDelegate,
};
use flui_view::prelude::{StatefulView, StatelessView, ViewState};
use flui_view::{BuildContext, IntoView, LifecycleContext, RebuildHandle, ViewExt};

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
/// mutating it.
///
/// # Stretch and snap are configured by the delegate
///
/// Over-scroll stretch is configured per delegate —
/// [`SliverPersistentHeaderDelegate::stretch_configuration`] — so this widget
/// carries no stretch knob of its own.
/// Floating headers use the enclosing [`Scrollable`](super::Scrollable)'s
/// activity transitions to start and interrupt the delegate's snap animation.
/// Outside that scope the header still floats, without gesture-driven snap.
#[derive(Clone, StatelessView)]
pub struct SliverPersistentHeader {
    delegate: Rc<dyn SliverPersistentHeaderDelegate>, // carries flui-view's SharedHeaderDelegate erasure (justified at its declaration) through the facade
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

/// Owner-local scroll edges and their latest command. The listener commits
/// this copy before scheduling a rebuild, so a reentrant wake sees the command.
#[derive(Clone, Copy, Default)]
struct SnapTriggerSlot {
    /// The last non-idle direction observed while scrolling — what the
    /// scroll that just ended was doing. Captured here because ending the
    /// scroll resets the position's own direction to `Idle` before
    /// listeners run.
    last_direction: Option<ScrollDirection>,
    /// Whether the previous notification observed an active scroll — what
    /// turns a level (`is_scrolling`) into the two edges (started/ended).
    was_scrolling: bool,
    /// Monotone stamp for [`SnapCommand`]s issued by this host.
    epoch: u64,
    /// The command the next build will carry, `None` until the first
    /// scroll edge.
    pending: Option<SnapCommand>,
}

struct FloatingHeaderHostState {
    snap_controller: Option<DrivenController>,
    position: Option<ScrollPosition>,
    activity_listener: Option<ListenerId>,
    slot: Rc<Cell<SnapTriggerSlot>>,
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
            position: None,
            activity_listener: None,
            slot: Rc::new(Cell::new(SnapTriggerSlot::default())),
        }
    }
}

impl FloatingHeaderHostState {
    /// (Re)subscribe the activity listener to `position`, detaching from any
    /// previous one first — the swap path a controller replacement takes.
    fn subscribe_to(&mut self, position: Option<ScrollPosition>, ctx: &dyn LifecycleContext) {
        if let (Some(old_position), Some(id)) =
            (self.position.take(), self.activity_listener.take())
        {
            old_position.remove_activity_listener(id);
        }
        let Some(position) = position else {
            return;
        };
        let rebuild: RebuildHandle = ctx.rebuild_handle();
        let slot = Rc::clone(&self.slot);
        let observed_position = position.clone();
        let id = position.add_activity_listener(Rc::new(move || {
            let mut trigger = slot.get();
            let changed = trigger.on_activity(
                observed_position.is_scrolling(),
                observed_position.user_scroll_direction(),
            );
            slot.set(trigger);
            if changed {
                rebuild.schedule(flui_view::RebuildReason::AnimationTick);
            }
        }));
        self.position = Some(position);
        self.activity_listener = Some(id);
    }
}

impl ViewState<FloatingHeaderHost> for FloatingHeaderHostState {
    fn init_state(&mut self, ctx: &dyn LifecycleContext) {
        // Everything below is lifecycle-only capability acquisition
        // (ADR-0018 / ADR-0021 discipline): the handles are taken HERE and
        // used later, from the listener.
        // The controller: built where the vsync lives. Duration is a
        // placeholder — `maybe_start_snap_animation` re-applies the snap
        // configuration's own duration on every start.
        let controller = AnimationController::builder(std::time::Duration::from_millis(200))
            .build_on(VsyncScope::maybe_of(ctx).as_ref());
        self.snap_controller = Some(controller);

        // The trigger: subscribe to the enclosing scrollable's activity.
        // `depend_on` (not `get`): a controller swap on the enclosing
        // Scrollable republishes the scope, and this dependency is what
        // routes that into `did_change_dependencies` so the listener can
        // follow the position. No scope (a header outside any Scrollable,
        // or in a purely programmatic scroll view) simply means no snap
        // trigger — the header still floats; it just never settles by
        // animation.
        let position = ctx.depend_on::<ScrollPositionScope, _>(|scope| scope.position().clone());
        self.subscribe_to(position, ctx);
    }

    fn did_change_dependencies(&mut self, ctx: &dyn LifecycleContext) {
        if let Some(controller) = self.snap_controller.as_mut()
            && let Err(error) = controller.rebind(VsyncScope::maybe_of(ctx).as_ref())
        {
            tracing::error!(%error, "FloatingHeader lost its frame registry");
        }
        let position = ctx.depend_on::<ScrollPositionScope, _>(|scope| scope.position().clone());
        let unchanged = match (&self.position, &position) {
            (Some(current), Some(new)) => current.ptr_eq(new),
            (None, None) => true,
            _ => false,
        };
        if !unchanged {
            self.subscribe_to(position, ctx);
        }
    }

    fn build(&self, view: &FloatingHeaderHost, _ctx: &dyn BuildContext) -> impl IntoView {
        let command = self.slot.get().pending;
        let controller = self
            .snap_controller
            .as_ref()
            .map(|owner| owner.controller().clone());
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
        self.snap_controller.take();
    }
}

impl SnapTriggerSlot {
    fn on_activity(&mut self, is_scrolling: bool, direction: ScrollDirection) -> bool {
        let was_scrolling = std::mem::replace(&mut self.was_scrolling, is_scrolling);
        let action = if is_scrolling {
            if direction != ScrollDirection::Idle {
                self.last_direction = Some(direction);
            }
            if was_scrolling {
                // Mid-scroll direction change: nothing edge-shaped to do.
                return false;
            }
            // A NEW scroll began: an in-flight snap must yield to the
            // finger immediately (on the rising edge of "is scrolling").
            SnapAction::Stop
        } else {
            // Scrolling just ended: the position's own direction is already
            // reset, so the captured one is the scroll that ended. No captured
            // direction (a programmatic jump) ⇒ no snap: snapping keys on user
            // gestures only.
            let Some(direction) = self.last_direction.take() else {
                return false;
            };
            SnapAction::Settle(direction)
        };
        let Some(epoch) = self.epoch.checked_add(1) else {
            return false;
        };
        self.epoch = epoch;
        self.pending = Some(SnapCommand { epoch, action });
        true
    }
}

#[cfg(test)]
mod tests {
    use super::{ScrollDirection, SnapAction, SnapCommand, SnapTriggerSlot};

    // The private seed reaches the otherwise unreachable identity boundary.
    #[test]
    fn exhausted_snap_epochs_never_reissue_a_command() {
        let mut slot = SnapTriggerSlot {
            epoch: u64::MAX - 1,
            ..SnapTriggerSlot::default()
        };
        assert!(slot.on_activity(true, ScrollDirection::Forward));
        let final_command = Some(SnapCommand {
            epoch: u64::MAX,
            action: SnapAction::Stop,
        });
        assert_eq!(slot.pending, final_command);
        for direction in [ScrollDirection::Forward, ScrollDirection::Reverse] {
            assert!(!slot.on_activity(false, ScrollDirection::Idle));
            assert!(!slot.on_activity(true, direction));
            assert_eq!(slot.pending, final_command);
        }
    }
}
