//! Animated Box — the animation engine driving the real pipeline on GPU.
//!
//! The window breathes between red and blue: an `AnimationController`
//! bouncing 0 → 1 → 0 forever, with the fill crossfading through Oklab
//! (perceptually uniform — no muddy gray midpoint like gamma-sRGB lerp).
//! The box itself fills the window: the root hands a bare render child
//! tight window constraints, exactly like a bare `ColoredBox` under
//! Flutter's root.
//!
//! The full production loop, no shortcuts:
//!
//! ```text
//! AnimationController::repeat(reverse) → Ticker on the GLOBAL Scheduler
//!   → ticker registration fires the scheduler's frame-scheduled hook
//!   → AppBinding::wake_frame wakes the platform
//!   → runner pumps handle_begin_frame/handle_draw_frame on the frame
//!   → value listener recolors the RenderColoredBox + marks it paint-dirty
//!   → next tick re-registers the ticker → next wake → …
//! ```
//!
//! The loop is self-sustaining and STOPS sustaining itself the moment
//! the controller stops — no busy-looping while idle.
//!
//! Run with: cargo run -p flui --example animated_box_app

use std::sync::Arc;
use std::time::Duration;

use flui_animation::{Animation, AnimationController};
use flui_app::{AppBinding, Scheduler, run_app};
use flui_foundation::{HasInstance, Listenable, RenderId};
use flui_objects::RenderColoredBox;
use flui_rendering::pipeline::PipelineOwner;
use flui_types::Color;
use flui_view::{BuildContext, IntoView, RenderView, StatelessView, View, ViewExt};

/// Leaf render view producing the box the animation will drive.
#[derive(Clone)]
struct AnimatedBox;

impl RenderView for AnimatedBox {
    type Protocol = flui_rendering::protocol::BoxProtocol;
    type RenderObject = RenderColoredBox;

    fn create_render_object(&self) -> Self::RenderObject {
        RenderColoredBox::red(60.0, 60.0)
    }

    fn update_render_object(&self, _render_object: &mut Self::RenderObject) {
        // This example never rebuilds the view tree; the animation
        // listener mutates the render object directly each tick.
    }
}

flui_view::impl_render_view!(AnimatedBox);

/// Stateless root that builds the box.
#[derive(Clone)]
struct App;

impl StatelessView for App {
    fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
        AnimatedBox.boxed()
    }
}

impl View for App {
    fn create_element(&self) -> flui_view::element::ElementKind {
        flui_view::element::ElementKind::stateless(self)
    }
}

/// Depth-first search for the demo's single [`RenderColoredBox`],
/// returning its id and depth.
///
/// Walked per tick instead of cached: the tree is three nodes, and a
/// fresh lookup stays correct across any churn (generational ids make a
/// stale cache miss, not alias).
fn find_colored_box(owner: &PipelineOwner) -> Option<(RenderId, usize)> {
    fn walk(owner: &PipelineOwner, id: RenderId, depth: usize) -> Option<(RenderId, usize)> {
        let node = owner.render_tree().get(id)?;
        let is_box = node.as_box().is_some_and(|entry| {
            entry
                .render_object()
                .as_any()
                .downcast_ref::<RenderColoredBox>()
                .is_some()
        });
        if is_box {
            return Some((id, depth));
        }
        let children: Vec<RenderId> = owner.render_tree().children(id).to_vec();
        children
            .into_iter()
            .find_map(|child| walk(owner, child, depth + 1))
    }
    walk(owner, owner.root_id()?, 0)
}

fn main() {
    // `Scheduler` is a cheap handle over shared `Arc` state, so cloning
    // the global singleton yields a handle onto the SAME registries the
    // runner pumps every frame — the controller's ticker actually ticks.
    let scheduler = Arc::new(Scheduler::instance().clone());
    let controller = AnimationController::new(Duration::from_millis(1400), scheduler);

    let red = Color::rgb(244, 67, 54);
    let blue = Color::rgb(33, 150, 243);

    let ticked = controller.clone();
    let _listener_id = controller.add_listener(Arc::new(move || {
        let value = ticked.value();
        let binding = AppBinding::instance();
        let mut owner = binding.render_pipeline_mut();

        let Some((id, depth)) = find_colored_box(&owner) else {
            // The first tick can land before the element tree mounts the
            // render object; skip until it exists.
            return;
        };
        if let Some(target) = owner
            .render_tree_mut()
            .get_mut(id)
            .and_then(|node| node.as_box_mut())
            .and_then(|entry| {
                entry
                    .render_object_mut()
                    .as_any_mut()
                    .downcast_mut::<RenderColoredBox>()
            })
        {
            target.set_color(Color::lerp_oklab(red, blue, value).to_f32_array());
        }
        // Color is paint-only state: invalidate paint, not layout. The
        // dirty mark fires the visual-update notifier, which wakes the
        // platform for the next frame.
        owner.add_node_needing_paint(id, depth);
    }));

    // Bounce 0 → 1 → 0 forever.
    controller
        .repeat(true)
        .expect("a freshly created controller accepts repeat()");

    run_app(App);
}
