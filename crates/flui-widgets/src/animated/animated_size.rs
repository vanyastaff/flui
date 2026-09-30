//! [`AnimatedSize`] — animates its own size toward its child's natural size
//! whenever that size changes.
//!
//! This widget is
//! structurally different from every sibling in this module
//! (`AnimatedOpacity`, `AnimatedAlign`, `AnimatedPadding`): those all delegate
//! their `build` to an existing plain widget (`Opacity`, `Align`, `Padding`)
//! wrapped in [`AnimatedBuilder`](crate::AnimatedBuilder), rebuilding the tree
//! every tick over a stateless render object. `RenderAnimatedSize` cannot work
//! that way — it must **persist** across rebuilds (it owns the retarget state
//! machine and drives its own layout via a self-dirty handle, per
//! `docs/adr/ADR-0013-render-object-attach-self-dirty-handle.md`), so this
//! widget's `build` returns a private [`RenderView`] directly, and
//! `update_render_object` reaches the persistent render object through
//! **targeted setters** — never `*render_object = ...` (that convention, used
//! by `Align`, would silently wipe the in-flight animation state on every
//! unrelated rebuild).

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use flui_animation::curve::{ArcCurve, Curve};
use flui_animation::{
    Animation, AnimationController, AnimationStatus, Curves, Vsync, VsyncRegistration,
};
use flui_foundation::ListenerId;
use flui_objects::RenderAnimatedSize;
use flui_painting::Alignment;
use flui_painting::paint::Clip;
use flui_rendering::protocol::BoxProtocol;
use flui_view::prelude::{BuildContext, LifecycleContext, StatefulView};
use flui_view::{
    BuildContextExt, Child, EventCx, EventOutcome, IntoView, LocalPostFrameHandle, RenderView,
    ViewState, WriterSource, impl_render_view,
};

type EndCallback = Rc<dyn Fn(&mut EventCx<'_>)>;

use crate::animated::vsync_scope::VsyncScope;

/// Animates its size toward its child's natural size whenever that size
/// changes, clipping overflow while the resize animation is in flight.
///
/// The first build sits at the child's size with no motion; each later
/// rebuild whose child reports a different size animates toward it over
/// `duration` along `curve`. See the module docs for why this widget does not
/// follow the sibling `AnimatedBuilder` convention.
#[derive(Clone, StatefulView)]
pub struct AnimatedSize {
    alignment: Alignment,
    duration: Duration,
    reverse_duration: Option<Duration>,
    curve: ArcCurve,
    clip_behavior: Clip,
    on_end: Option<EndCallback>,
    child: Child,
}

impl AnimatedSize {
    /// Animates size changes over `duration`, with `Alignment::CENTER`,
    /// `Curves::Linear` (deliberately NOT the sibling widgets' `EaseInOut`
    /// default), and
    /// `Clip::HardEdge`. The child is optional: a childless `AnimatedSize`
    /// exercises the tight/no-child fast path, a real configuration.
    pub fn new(duration: Duration) -> Self {
        Self {
            alignment: Alignment::CENTER,
            duration,
            reverse_duration: None,
            curve: ArcCurve::new(Curves::Linear),
            clip_behavior: Clip::HardEdge,
            on_end: None,
            child: Child::empty(),
        }
    }

    /// Sets the child.
    #[must_use]
    pub fn child(mut self, child: impl IntoView) -> Self {
        self.child = Child::some(child.into_view());
        self
    }

    /// Overrides the alignment used while the child is smaller than the
    /// animated box.
    #[must_use]
    pub fn alignment(mut self, alignment: Alignment) -> Self {
        self.alignment = alignment;
        self
    }

    /// Overrides the reverse-run duration. Confirmed inert for
    /// `RenderAnimatedSize` today — it only ever drives its controller
    /// forward, never `.reverse()` — kept so the constructor mirrors
    /// the `AnimationController` API.
    #[must_use]
    pub fn reverse_duration(mut self, reverse_duration: Duration) -> Self {
        self.reverse_duration = Some(reverse_duration);
        self
    }

    /// Overrides the easing curve; accepts any type implementing [`Curve`].
    #[must_use]
    pub fn curve(mut self, curve: impl Curve + Send + Sync + 'static) -> Self {
        self.curve = ArcCurve::new(curve);
        self
    }

    /// Overrides the clip behavior applied while the animated size is
    /// smaller than the child (`Clip::HardEdge` by default).
    #[must_use]
    pub fn clip_behavior(mut self, clip_behavior: Clip) -> Self {
        self.clip_behavior = clip_behavior;
        self
    }

    /// Sets a callback fired each time a resize run completes.
    #[must_use]
    pub fn on_end<R: EventOutcome>(
        mut self,
        on_end: impl Fn(&mut EventCx<'_>) -> R + 'static,
    ) -> Self {
        self.on_end = Some(Rc::new(move |cx| on_end(cx).report()));
        self
    }
}

impl std::fmt::Debug for AnimatedSize {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AnimatedSize")
            .field("alignment", &self.alignment)
            .field("duration", &self.duration)
            .field("clip_behavior", &self.clip_behavior)
            .finish_non_exhaustive()
    }
}

/// State for [`AnimatedSize`] — owns the persistent [`AnimationController`]
/// that `RenderAnimatedSize` subscribes to directly in its own `attach`
/// (the render object is handed an already-built controller and never sees
/// a `Vsync`/`UpdateScheduler` itself).
pub struct AnimatedSizeState {
    controller: AnimationController,
    vsync: Option<Vsync>,
    vsync_registration: Option<VsyncRegistration>,
    status_listener_id: Option<ListenerId>,
    completed_runs: Arc<AtomicU64>,
    delivered_completed_runs: Cell<u64>,
    writer: Option<WriterSource>,
    post_frame: Option<LocalPostFrameHandle>,
    mounted: Rc<Cell<bool>>,
    on_end: Rc<RefCell<Option<EndCallback>>>,
    child: Child,
}

impl std::fmt::Debug for AnimatedSizeState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AnimatedSizeState")
            .field("registered", &self.vsync_registration.is_some())
            .finish_non_exhaustive()
    }
}

impl StatefulView for AnimatedSize {
    type State = AnimatedSizeState;

    fn create_state(&self) -> Self::State {
        // A real, but permanently detached, ticker -- not `without_ticker`:
        // this controller's `is_animating()` is read by `RenderAnimatedSize`
        // (`flui-objects`), and `is_animating` is intentionally
        // ticker-based, not status-based
        // — a ticker-less controller can never report `is_animating() ==
        // true`. `VsyncScope` still drives the actual value ticks
        // deterministically via `tick_at`; `with_detached_ticker` gives this
        // controller a ticker whose `start()`/`stop()`/`mute()` transition
        // real ticker state without needing an `UpdateScheduler` at all — no
        // allocation for something nothing was ever going to pump.
        let controller = AnimationController::with_detached_ticker(self.duration);
        if let Some(reverse_duration) = self.reverse_duration {
            controller.set_reverse_duration(reverse_duration);
        }
        AnimatedSizeState {
            controller,
            vsync: None,
            vsync_registration: None,
            status_listener_id: None,
            completed_runs: Arc::new(AtomicU64::new(0)),
            delivered_completed_runs: Cell::new(0),
            writer: None,
            post_frame: None,
            mounted: Rc::new(Cell::new(true)),
            on_end: Rc::new(RefCell::new(self.on_end.clone())),
            child: self.child.clone(),
        }
    }
}

impl ViewState<AnimatedSize> for AnimatedSizeState {
    fn init_state(&mut self, ctx: &dyn LifecycleContext) {
        self.writer = Some(ctx.writer_source());
        self.post_frame = ctx.local_post_frame_handle();
        let completed_runs = Arc::clone(&self.completed_runs);
        let rebuild = ctx.rebuild_handle();
        self.status_listener_id =
            Some(self.controller.add_status_listener(Arc::new(move |status| {
                if status == AnimationStatus::Completed {
                    completed_runs.fetch_add(1, Ordering::SeqCst);
                    rebuild.schedule(flui_view::RebuildReason::AnimationTick);
                }
            })));

        if let Some(vsync) = ctx.get::<VsyncScope, _>(|scope| scope.vsync().clone()) {
            self.vsync_registration = Some(vsync.register(self.controller.clone()));
            self.vsync = Some(vsync);
        }
    }

    fn build(&self, view: &AnimatedSize, _ctx: &dyn BuildContext) -> impl IntoView {
        let completed_runs = self.completed_runs.load(Ordering::SeqCst);
        let delivered_runs = self.delivered_completed_runs.get();
        if completed_runs > delivered_runs {
            self.delivered_completed_runs.set(completed_runs);
            if self.on_end.borrow().is_some() {
                if let Some(post_frame) = &self.post_frame {
                    let writer = self
                        .writer
                        .clone()
                        .expect("BUG: AnimatedSize initialized before build");
                    let mounted = self.mounted.clone();
                    let callback = self.on_end.clone();
                    if let Err(error) = post_frame.schedule_local(move |_| {
                        for _ in delivered_runs..completed_runs {
                            if !mounted.get() {
                                break;
                            }
                            let on_end = callback.borrow().clone();
                            if let Some(on_end) = on_end {
                                writer.write(|cx| on_end(cx));
                            }
                        }
                    }) {
                        tracing::warn!(
                            ?error,
                            "AnimatedSize: completion dropped because the owner post-frame lane is closed"
                        );
                    }
                } else {
                    tracing::warn!(
                        "AnimatedSize: completion dropped because there is no owner post-frame lane"
                    );
                }
            }
        }

        AnimatedSizeRenderView {
            controller: self.controller.clone(),
            curve: view.curve.clone(),
            alignment: view.alignment,
            clip_behavior: view.clip_behavior,
            child: self.child.clone(),
        }
    }

    fn did_update_view(&mut self, _old_view: &AnimatedSize, new_view: &AnimatedSize) {
        let previous = self.on_end.replace(new_view.on_end.clone());
        drop(previous);
        self.child = new_view.child.clone();
        // Plain-assignment setters — no restart of an in-flight run.
        self.controller.set_duration(new_view.duration);
        if let Some(reverse_duration) = new_view.reverse_duration {
            self.controller.set_reverse_duration(reverse_duration);
        }
    }

    fn dispose(&mut self) {
        self.mounted.set(false);
        if let Some(id) = self.status_listener_id.take() {
            self.controller.remove_status_listener(id);
        }
        if let (Some(vsync), Some(registration)) = (&self.vsync, self.vsync_registration) {
            vsync.unregister(registration);
        }
        self.controller.dispose();
    }
}

/// Private render-view wrapper around the persistent [`RenderAnimatedSize`].
///
/// See the module docs: unlike every sibling, this does not delegate through
/// `AnimatedBuilder`, and [`update_render_object`](RenderView::update_render_object)
/// uses targeted setters (never replaces the render object), because
/// `RenderAnimatedSize` persists its retarget state and listener
/// subscriptions across rebuilds.
#[derive(Clone)]
struct AnimatedSizeRenderView {
    controller: AnimationController,
    curve: ArcCurve,
    alignment: Alignment,
    clip_behavior: Clip,
    child: Child,
}

impl std::fmt::Debug for AnimatedSizeRenderView {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AnimatedSizeRenderView")
            .field("alignment", &self.alignment)
            .field("clip_behavior", &self.clip_behavior)
            .finish_non_exhaustive()
    }
}

impl RenderView for AnimatedSizeRenderView {
    type Protocol = BoxProtocol;
    type RenderObject = RenderAnimatedSize;

    fn create_render_object(
        &self,
        _ctx: &flui_view::RenderObjectContext<'_>,
    ) -> Self::RenderObject {
        RenderAnimatedSize::new(
            self.controller.clone(),
            self.curve.clone(),
            self.alignment,
            self.clip_behavior,
        )
    }

    fn update_render_object(
        &self,
        _ctx: &flui_view::RenderObjectContext<'_>,
        render_object: &mut Self::RenderObject,
    ) -> flui_rendering::RenderUpdateImpact {
        let mut impact = flui_rendering::RenderUpdateImpact::NONE; // Targeted setters only. `render_object` is the SAME persistent
        // instance across every rebuild (only `create_render_object` builds a
        // new one) — the controller is not re-passed here, it is the
        // Arc-backed object the render object already holds; `did_update_view`
        // pushes its duration directly.
        impact |= render_object.set_alignment(self.alignment);
        impact |= render_object.set_curve(self.curve.clone());
        impact |= render_object.set_clip_behavior(self.clip_behavior);
        impact
    }

    flui_view::single_child_view_children!();
}

impl_render_view!(AnimatedSizeRenderView);
