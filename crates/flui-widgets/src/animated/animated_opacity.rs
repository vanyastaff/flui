//! [`AnimatedOpacity`] — animates its child's opacity when the target changes.
//!
//! Unlike its sibling implicit-animation widgets (`AnimatedPadding`,
//! `AnimatedAlign`), this widget does not rebuild its child through an
//! [`AnimatedBuilder`](crate::AnimatedBuilder) each tick — see
//! `AnimatedSize`'s module docs
//! (`crates/flui-widgets/src/animated/animated_size.rs`) for why that
//! pattern is wrong for a widget whose render object must persist across
//! ticks. `build()` instead returns a private [`RenderView`] wrapper around
//! the persistent [`RenderAnimatedOpacity`],
//! injected with a [`ProxyAnimation<f32>`] the state owns; a tick updates
//! alpha and repaints without ever re-entering the widget tree, and a
//! retarget (`did_update_view`) swaps the proxy's parent instead of
//! replacing the render object.

use std::sync::Arc;
use std::time::Duration;

use flui_animation::curve::{ArcCurve, Curve};
use flui_animation::{AnimatableExt, Animation, ProxyAnimation};
use flui_objects::RenderAnimatedOpacity;
use flui_rendering::protocol::BoxProtocol;
use flui_view::prelude::{BuildContext, StatefulView};
use flui_view::{
    BoxedView, BuildContextExt, IntoView, RenderObjectContext, RenderView, View, ViewExt,
    ViewState, impl_render_view,
};

use crate::animated::implicitly_animated::{DEFAULT_DURATION, ImplicitAnimation, default_curve};
use crate::animated::vsync_scope::VsyncScope;

/// Animates the opacity of its child whenever a new `opacity` is given.
///
/// Flutter parity: `widgets/implicit_animations.dart` `AnimatedOpacity`. On the
/// first build the child sits at the given opacity with no motion; each later
/// build with a *different* opacity animates from the current value to the new
/// one over `duration` along `curve`. The child is always laid out — only its
/// painting fades.
///
/// Driven deterministically by a binding when a
/// [`VsyncScope`] is above it; otherwise driven by its own
/// scheduler ticker on a real display.
#[derive(Clone, StatefulView)]
pub struct AnimatedOpacity {
    opacity: f32,
    duration: Duration,
    curve: ArcCurve,
    child: BoxedView,
}

impl AnimatedOpacity {
    /// Animate `child` toward `opacity` (`0.0` transparent … `1.0` opaque),
    /// with the 200 ms default duration and an ease-in-out curve.
    pub fn new(opacity: f32, child: impl IntoView) -> Self {
        Self {
            opacity,
            duration: DEFAULT_DURATION,
            curve: default_curve(),
            child: child.into_view().boxed(),
        }
    }

    /// Override the transition duration.
    #[must_use]
    pub fn duration(mut self, duration: Duration) -> Self {
        self.duration = duration;
        self
    }

    /// Override the easing curve; accepts any type implementing
    /// [`Curve`], including elastic and bounce curves
    /// from [`flui_animation::Curves`].
    #[must_use]
    pub fn curve(mut self, curve: impl Curve + Send + Sync + 'static) -> Self {
        self.curve = ArcCurve::new(curve);
        self
    }
}

impl std::fmt::Debug for AnimatedOpacity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AnimatedOpacity")
            .field("opacity", &self.opacity)
            .field("duration", &self.duration)
            .finish_non_exhaustive()
    }
}

/// The tween composed over the curved controller — the value stream
/// [`RenderAnimatedOpacity`] actually observes. Rebuilt from `animation`'s
/// current tween/curve on every call (cheap: `Tween`/`CurvedAnimation` clones
/// are `Arc`-backed); the caller wraps the result in the state's
/// [`ProxyAnimation<f32>`] at construction, then swaps it in as the proxy's
/// new parent on every retarget (`ProxyAnimation::set_parent`) — the render
/// object never sees the swap, only the proxy's re-fired notification. See
/// `RenderAnimatedOpacity`'s module docs' *Retargeting* section.
fn compose_animation(animation: &ImplicitAnimation<f32>) -> Arc<dyn Animation<f32>> {
    let curved: Arc<dyn Animation<f32>> = Arc::new(animation.curved());
    Arc::new(animation.tween().animate(curved))
}

/// State for [`AnimatedOpacity`] — owns the persistent opacity animation and
/// the [`ProxyAnimation<f32>`] injected into the persistent render object.
#[derive(Debug)]
pub struct AnimatedOpacityState {
    animation: ImplicitAnimation<f32>,
    proxy: ProxyAnimation<f32>,
    child: BoxedView,
}

impl StatefulView for AnimatedOpacity {
    type State = AnimatedOpacityState;

    fn create_state(&self) -> Self::State {
        let animation = ImplicitAnimation::new(self.opacity, self.duration, self.curve.clone());
        let proxy = ProxyAnimation::new(compose_animation(&animation));
        AnimatedOpacityState {
            animation,
            proxy,
            child: self.child.clone(),
        }
    }
}

impl ViewState<AnimatedOpacity> for AnimatedOpacityState {
    fn init_state(&mut self, ctx: &dyn BuildContext) {
        if let Some(vsync) = ctx.get::<VsyncScope, _>(|scope| scope.vsync().clone()) {
            self.animation.register(vsync);
        }
    }

    fn build(&self, _view: &AnimatedOpacity, _ctx: &dyn BuildContext) -> impl IntoView {
        AnimatedOpacityRenderView {
            proxy: self.proxy.clone(),
            child: self.child.clone(),
        }
    }

    fn did_update_view(&mut self, _old_view: &AnimatedOpacity, new_view: &AnimatedOpacity) {
        self.child = new_view.child.clone();
        // Current-value-as-new-start retarget algebra + restart-from-0, plus
        // the curve-swap-without-restart path, live entirely in
        // `ImplicitAnimation::retarget` (a no-op on both fronts when neither
        // the target nor the curve changed, so an unrelated rebuild does
        // nothing to the run in flight).
        let changed =
            self.animation
                .retarget(new_view.opacity, new_view.duration, new_view.curve.clone());
        // Recompose over the (possibly just-retargeted) tween/curve and hand
        // the render object's proxy its new parent — this is the ENTIRE
        // retarget path the render object observes; it never sees the
        // controller, tween, or curve directly. Gated on `changed` so a
        // child-only rebuild does not reallocate the tween/curved chain and
        // drop/re-add the proxy's controller subscription for nothing.
        if changed {
            self.proxy.set_parent(compose_animation(&self.animation));
        }
    }

    fn dispose(&mut self) {
        self.animation.dispose();
    }
}

/// Private render-view wrapper around the persistent [`RenderAnimatedOpacity`].
///
/// Mirrors `AnimatedSizeRenderView`'s shape
/// (`crates/flui-widgets/src/animated/animated_size.rs`):
/// `create_render_object` injects the state's [`ProxyAnimation<f32>`] once.
/// Unlike that sibling, `update_render_object` has no targeted setters to
/// call — the proxy instance is the SAME `Arc`-backed object across every
/// rebuild, and retargeting flows entirely through `proxy.set_parent` on the
/// state side (`did_update_view` above), so there is nothing new to push
/// into the render object from here.
#[derive(Clone)]
struct AnimatedOpacityRenderView {
    proxy: ProxyAnimation<f32>,
    child: BoxedView,
}

impl std::fmt::Debug for AnimatedOpacityRenderView {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AnimatedOpacityRenderView")
            .finish_non_exhaustive()
    }
}

impl RenderView for AnimatedOpacityRenderView {
    type Protocol = BoxProtocol;
    type RenderObject = RenderAnimatedOpacity;

    fn create_render_object(&self, _ctx: &RenderObjectContext<'_>) -> Self::RenderObject {
        // Flutter default: `alwaysIncludeSemantics = false`. `AnimatedOpacity`
        // does not expose a builder for it yet — no call site needs it — so
        // this is not a widget-configurable knob today.
        RenderAnimatedOpacity::new(self.proxy.clone(), false)
    }

    fn update_render_object(
        &self,
        _ctx: &RenderObjectContext<'_>,
        _render_object: &mut Self::RenderObject,
    ) {
        // Intentionally empty — see the struct doc.
    }

    fn has_children(&self) -> bool {
        true
    }

    fn visit_child_views(&self, visitor: &mut dyn FnMut(&dyn View)) {
        visitor(&self.child);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::SizedBox;

    fn probe(opacity: f32) -> AnimatedOpacity {
        AnimatedOpacity::new(opacity, SizedBox::shrink())
    }

    /// The gating this test pins: `did_update_view` must only call
    /// `ProxyAnimation::set_parent` when `ImplicitAnimation::retarget`
    /// reports an actual change. `parent()` returns the proxy's CURRENT
    /// parent `Arc`; an unrelated reconfigure (same opacity, duration, and
    /// curve as construction — the default, cached via `default_curve()` so
    /// its `ArcCurve` compares reference-equal across the two `new()` calls
    /// below) must leave that `Arc` pointer untouched.
    #[test]
    fn unrelated_rebuild_does_not_swap_the_proxy_parent() {
        let view = probe(0.5);
        let mut state = view.create_state();
        let parent_before = state.proxy.parent();

        let unchanged = probe(0.5);
        state.did_update_view(&view, &unchanged);

        let parent_after = state.proxy.parent();
        assert!(
            Arc::ptr_eq(&parent_before, &parent_after),
            "an unrelated rebuild (same opacity/duration/curve) must not \
             hot-swap the proxy's parent animation",
        );
    }

    /// The flip side: a genuine opacity retarget MUST swap the proxy's
    /// parent, or the render object would keep observing the stale
    /// tween/curve composition.
    #[test]
    fn opacity_retarget_swaps_the_proxy_parent() {
        let view = probe(0.5);
        let mut state = view.create_state();
        let parent_before = state.proxy.parent();

        let retargeted = probe(0.9);
        state.did_update_view(&view, &retargeted);

        let parent_after = state.proxy.parent();
        assert!(
            !Arc::ptr_eq(&parent_before, &parent_after),
            "a genuine opacity retarget must swap the proxy's parent animation",
        );
    }

    /// And a curve-only change (opacity/duration unchanged) must ALSO swap
    /// the proxy's parent — the render object must observe the new curve,
    /// not the stale one baked into the old composition.
    #[test]
    fn curve_only_change_swaps_the_proxy_parent() {
        use flui_animation::Curves;

        let view = probe(0.5);
        let mut state = view.create_state();
        let parent_before = state.proxy.parent();

        let recurved = AnimatedOpacity::new(0.5, SizedBox::shrink()).curve(Curves::Linear);
        state.did_update_view(&view, &recurved);

        let parent_after = state.proxy.parent();
        assert!(
            !Arc::ptr_eq(&parent_before, &parent_after),
            "a curve-only change must swap the proxy's parent animation",
        );
    }
}

impl_render_view!(AnimatedOpacityRenderView);
