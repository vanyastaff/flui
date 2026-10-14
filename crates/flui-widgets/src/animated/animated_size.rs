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

use flui_animation::{MotionSpec, SpringDescription};
use flui_objects::{RenderAnimatedSize, SizeMotion, SizeMotionHandle};
use flui_painting::Alignment;
use flui_painting::paint::Clip;
use flui_rendering::protocol::BoxProtocol;
use flui_view::prelude::{BuildContext, LifecycleContext, StatefulView};
use flui_view::{Child, EventCx, EventOutcome, IntoView, RenderView, ViewState, impl_render_view};
use std::cell::{Cell, RefCell};
use std::rc::Rc;

type EndCallback = Rc<dyn Fn(&mut EventCx<'_>)>;

use crate::animated::implicitly_animated::MotionBuilder;
use crate::animated::vsync_scope::VsyncScope;

/// Animates its size toward its child's natural size whenever that size
/// changes, clipping overflow while the resize animation is in flight.
///
/// The first build sits at the child's size with no motion; each later
/// layout whose child reports a different size animates toward it from the
/// size shown, by a critically damped spring unless [`MotionBuilder`] says
/// otherwise. See the module docs for why this widget does not follow the
/// sibling `AnimatedBuilder` convention.
#[derive(Clone, StatefulView)]
pub struct AnimatedSize {
    alignment: Alignment,
    motion: MotionSpec,
    clip_behavior: Clip,
    on_end: Option<EndCallback>,
    child: Child,
}

impl AnimatedSize {
    /// Animates size changes by a critically damped spring of stiffness 400
    /// (a size settles without overshoot, as a content resize should), with
    /// `Alignment::CENTER` and `Clip::HardEdge`. The child is optional: a
    /// childless `AnimatedSize` exercises the tight/no-child fast path, a
    /// real configuration.
    pub fn new() -> Self {
        Self {
            alignment: Alignment::CENTER,
            motion: MotionSpec::Spring(
                SpringDescription::with_damping_ratio(1.0, 400.0, 1.0)
                    .expect("BUG: a unit-mass critically damped spring is valid"),
            ),
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

impl Default for AnimatedSize {
    fn default() -> Self {
        Self::new()
    }
}

impl MotionBuilder for AnimatedSize {
    fn motion_mut(&mut self) -> &mut MotionSpec {
        &mut self.motion
    }
}

impl std::fmt::Debug for AnimatedSize {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AnimatedSize")
            .field("alignment", &self.alignment)
            .field("motion", &self.motion)
            .field("clip_behavior", &self.clip_behavior)
            .finish_non_exhaustive()
    }
}

/// State for [`AnimatedSize`] — owns the [`SizeMotion`] that moves the shown
/// size. `RenderAnimatedSize` receives only a handle: it retargets the size
/// from layout and subscribes to it in its own `attach`, but never sees a
/// `Vsync`, and the state binds, reconfigures and disposes it.
pub struct AnimatedSizeState {
    size: SizeMotion,
    mounted: Rc<Cell<bool>>,
    on_end: Rc<RefCell<Option<EndCallback>>>,
    child: Child,
}

impl std::fmt::Debug for AnimatedSizeState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AnimatedSizeState")
            .field("size", &self.size)
            .finish_non_exhaustive()
    }
}

impl StatefulView for AnimatedSize {
    type State = AnimatedSizeState;

    fn create_state(&self) -> Self::State {
        AnimatedSizeState {
            size: SizeMotion::new(self.motion.clone(), None),
            mounted: Rc::new(Cell::new(true)),
            on_end: Rc::new(RefCell::new(self.on_end.clone())),
            child: self.child.clone(),
        }
    }
}

impl ViewState<AnimatedSize> for AnimatedSizeState {
    fn init_state(&mut self, ctx: &dyn LifecycleContext) {
        let writer = ctx.writer_source();
        let post_frame = ctx.post_frame_handle();
        let mounted = Rc::clone(&self.mounted);
        let callback = Rc::clone(&self.on_end);
        self.size.set_on_complete(Some(Rc::new(move || {
                if !mounted.get() {
                    return;
                }
                if let Some(post_frame) = &post_frame {
                    let writer = writer.clone();
                    let mounted = Rc::clone(&mounted);
                    let callback = Rc::clone(&callback);
                    if let Err(error) = post_frame.schedule(move |_| {
                        if mounted.get() {
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
                } else if callback.borrow().is_some() {
                    tracing::warn!(
                        "AnimatedSize: completion dropped because there is no owner post-frame lane"
                    );
                }
        })));

        self.did_change_dependencies(ctx);
    }

    fn did_change_dependencies(&mut self, ctx: &dyn LifecycleContext) {
        if let Err(error) = self.size.rebind(VsyncScope::maybe_of(ctx).as_ref()) {
            tracing::error!(%error, "AnimatedSize lost its frame registry");
        }
    }

    fn build(&self, view: &AnimatedSize, _ctx: &dyn BuildContext) -> impl IntoView {
        AnimatedSizeRenderView {
            size: self.size.handle(),
            alignment: view.alignment,
            clip_behavior: view.clip_behavior,
            child: self.child.clone(),
        }
    }

    fn did_update_view(&mut self, _old_view: &AnimatedSize, new_view: &AnimatedSize) {
        // A motion-only change carries a resize in flight on from the size
        // and velocity shown.
        self.size.set_motion(new_view.motion.clone());
        let previous = self.on_end.replace(new_view.on_end.clone());
        drop(previous);
        self.child = new_view.child.clone();
    }

    fn dispose(&mut self) {
        self.mounted.set(false);
        self.size.set_on_complete(None);
        self.size.dispose();
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
    size: SizeMotionHandle,
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
        RenderAnimatedSize::new(self.size.clone(), self.alignment, self.clip_behavior)
    }

    fn update_render_object(
        &self,
        _ctx: &flui_view::RenderObjectContext<'_>,
        render_object: &mut Self::RenderObject,
    ) -> flui_rendering::RenderUpdateImpact {
        // Targeted setters only: `render_object` is the SAME persistent
        // instance across every rebuild (only `create_render_object` builds a
        // new one). The size handle is not re-passed; the state's
        // `SizeMotion` behind it is the same, and the state carries motion
        // changes to it.
        let mut impact = flui_rendering::RenderUpdateImpact::NONE;
        impact |= render_object.set_alignment(self.alignment);
        impact |= render_object.set_clip_behavior(self.clip_behavior);
        impact
    }

    flui_view::single_child_view_children!();
}

impl_render_view!(AnimatedSizeRenderView);
