//! [`AnimatedSize`] — animates its own size toward its child's natural size
//! whenever that size changes.
//!
//! Flutter parity: `widgets/animated_size.dart` `AnimatedSize`. This widget is
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

use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use flui_animation::curve::{ArcCurve, Curve};
use flui_animation::{
    Animation, AnimationController, AnimationStatus, Curves, Scheduler, Vsync, VsyncRegistration,
};
use flui_foundation::ListenerId;
use flui_objects::RenderAnimatedSize;
use flui_rendering::protocol::BoxProtocol;
use flui_types::{Alignment, painting::Clip};
use flui_view::prelude::{BuildContext, StatefulView};
use flui_view::{BuildContextExt, Child, IntoView, RenderView, View, ViewState, impl_render_view};

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
    on_end: Option<Rc<dyn Fn()>>,
    child: Child,
}

impl AnimatedSize {
    /// Animates size changes over `duration`, with `Alignment::CENTER`,
    /// `Curves::Linear` (oracle parity — `widgets/animated_size.dart:33` —
    /// deliberately NOT the sibling widgets' `EaseInOut` default), and
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
    /// forward, never `.reverse()` — kept for constructor/widget parity with
    /// Flutter's `AnimationController` API.
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
    pub fn on_end(mut self, on_end: impl Fn() + 'static) -> Self {
        self.on_end = Some(Rc::new(on_end));
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
/// a `Vsync`/`Scheduler` itself).
pub struct AnimatedSizeState {
    controller: AnimationController,
    vsync: Option<Vsync>,
    vsync_registration: Option<VsyncRegistration>,
    status_listener_id: Option<ListenerId>,
    completed_runs: Arc<AtomicU64>,
    delivered_completed_runs: Cell<u64>,
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
        // A fresh, never-pumped scheduler: on a real display its ticker would
        // drive the controller off wall-clock time; under a `VsyncScope` the
        // binding drives it deterministically via `tick_at` instead, so the
        // two paths never double-advance (mirrors `ImplicitController::new`,
        // `crate::animated::implicitly_animated`).
        let controller = AnimationController::new(self.duration, Arc::new(Scheduler::new()));
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
            child: self.child.clone(),
        }
    }
}

impl ViewState<AnimatedSize> for AnimatedSizeState {
    fn init_state(&mut self, ctx: &dyn BuildContext) {
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
            if let Some(on_end) = view.on_end.as_ref() {
                for _ in delivered_runs..completed_runs {
                    on_end();
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
        self.child = new_view.child.clone();
        // Plain-assignment setters, matching the oracle — no restart of an
        // in-flight run.
        self.controller.set_duration(new_view.duration);
        if let Some(reverse_duration) = new_view.reverse_duration {
            self.controller.set_reverse_duration(reverse_duration);
        }
    }

    fn dispose(&mut self) {
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
    ) {
        // Targeted setters only. `render_object` is the SAME persistent
        // instance across every rebuild (only `create_render_object` builds a
        // new one) — the controller is not re-passed here, it is the
        // Arc-backed object the render object already holds; `did_update_view`
        // pushes its duration directly.
        render_object.set_alignment(self.alignment);
        render_object.set_curve(self.curve.clone());
        render_object.set_clip_behavior(self.clip_behavior);
    }

    fn has_children(&self) -> bool {
        self.child.is_some()
    }

    fn visit_child_views(&self, visitor: &mut dyn FnMut(&dyn View)) {
        if let Some(child) = self.child.as_ref() {
            visitor(child);
        }
    }
}

impl_render_view!(AnimatedSizeRenderView);

#[cfg(test)]
mod tests {
    use flui_view::RenderView;

    use super::*;
    use crate::SizedBox;

    fn render_view(alignment: Alignment, clip_behavior: Clip) -> AnimatedSizeRenderView {
        let controller =
            AnimationController::new(Duration::from_millis(100), Arc::new(Scheduler::new()));
        AnimatedSizeRenderView {
            controller,
            curve: ArcCurve::new(Curves::Linear),
            alignment,
            clip_behavior,
            child: Child::empty(),
        }
    }

    #[test]
    fn create_render_object_installs_the_given_alignment_and_clip_behavior() {
        let render_object = render_view(Alignment::BOTTOM_RIGHT, Clip::None)
            .create_render_object(&flui_view::RenderObjectContext::detached());
        assert_eq!(render_object.alignment(), Alignment::BOTTOM_RIGHT);
        assert_eq!(render_object.clip_behavior(), Clip::None);
    }

    #[test]
    fn update_render_object_reconfigures_alignment_and_clip_behavior_via_targeted_setters() {
        let mut render_object = render_view(Alignment::CENTER, Clip::HardEdge)
            .create_render_object(&flui_view::RenderObjectContext::detached());
        assert_eq!(render_object.alignment(), Alignment::CENTER);

        render_view(Alignment::TOP_LEFT, Clip::AntiAlias).update_render_object(
            &flui_view::RenderObjectContext::detached(),
            &mut render_object,
        );

        assert_eq!(render_object.alignment(), Alignment::TOP_LEFT);
        assert_eq!(render_object.clip_behavior(), Clip::AntiAlias);
    }

    #[test]
    fn has_children_reflects_whether_a_child_was_set() {
        let mut without_child = render_view(Alignment::CENTER, Clip::HardEdge);
        assert!(!without_child.has_children());

        without_child.child = Child::some(SizedBox::shrink().into_view());
        assert!(without_child.has_children());
    }

    #[test]
    fn render_view_debug_reports_alignment_and_clip_behavior() {
        let debug = format!("{:?}", render_view(Alignment::CENTER, Clip::HardEdge));
        assert!(
            debug.contains("alignment:") && debug.contains("clip_behavior:"),
            "Debug output must include alignment and clip_behavior, got: {debug}",
        );
    }

    #[test]
    fn new_defaults_to_center_alignment_hard_edge_clip_no_child() {
        let widget = AnimatedSize::new(Duration::from_millis(50));
        assert_eq!(widget.alignment, Alignment::CENTER);
        assert_eq!(widget.clip_behavior, Clip::HardEdge);
        assert!(!widget.child.is_some());
    }

    #[test]
    fn builder_methods_override_alignment_and_clip_behavior() {
        let widget = AnimatedSize::new(Duration::from_millis(50))
            .alignment(Alignment::TOP_LEFT)
            .clip_behavior(Clip::None);
        assert_eq!(widget.alignment, Alignment::TOP_LEFT);
        assert_eq!(widget.clip_behavior, Clip::None);
    }
}
