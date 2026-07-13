//! `Visibility` -- show/hide a child, optionally preserving its state via
//! `Offstage`. Verifies the build branches and animation policy documented in
//! `crates/flui-widgets/src/interaction/visibility.rs` (Flutter oracle:
//! `widgets/indexed_stack.dart`).

mod common;

use std::any::TypeId;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::Duration;

use common::{lay_out, lay_out_animated, loose, size};
use flui_animation::{Animation, AnimationController, Vsync, VsyncRegistration};
use flui_scheduler::Scheduler;
use flui_view::prelude::{BuildContext, StatefulView, StatelessView};
use flui_view::{
    BoxedView, BuildContextExt, BuildOwner, ElementTree, ErrorView, IntoView, ViewExt, ViewState,
};
use flui_widgets::{SizedBox, TickerMode, Visibility, VsyncScope};
use parking_lot::Mutex;

const FRAME: Duration = Duration::from_millis(20);

#[derive(Clone, StatefulView)]
struct AnimationProbe {
    controller: AnimationController,
    found_ambient: Arc<Mutex<Option<bool>>>,
    init_count: Arc<AtomicUsize>,
    dispose_count: Arc<AtomicUsize>,
}

struct AnimationProbeState {
    controller: AnimationController,
    found_ambient: Arc<Mutex<Option<bool>>>,
    init_count: Arc<AtomicUsize>,
    dispose_count: Arc<AtomicUsize>,
    registration: Option<(Vsync, VsyncRegistration)>,
}

impl StatefulView for AnimationProbe {
    type State = AnimationProbeState;

    fn create_state(&self) -> Self::State {
        AnimationProbeState {
            controller: self.controller.clone(),
            found_ambient: Arc::clone(&self.found_ambient),
            init_count: Arc::clone(&self.init_count),
            dispose_count: Arc::clone(&self.dispose_count),
            registration: None,
        }
    }
}

impl std::fmt::Debug for AnimationProbeState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AnimationProbeState")
            .finish_non_exhaustive()
    }
}

impl ViewState<AnimationProbe> for AnimationProbeState {
    fn init_state(&mut self, ctx: &dyn BuildContext) {
        self.init_count.fetch_add(1, Ordering::Relaxed);
        let ambient = ctx.get::<VsyncScope, _>(|scope| scope.vsync().clone());
        *self.found_ambient.lock() = Some(ambient.is_some());
        if let Some(vsync) = ambient {
            let registration = vsync.register(self.controller.clone());
            self.registration = Some((vsync, registration));
        }
    }

    fn dispose(&mut self) {
        self.dispose_count.fetch_add(1, Ordering::Relaxed);
        if let Some((vsync, registration)) = self.registration.take() {
            vsync.unregister(registration);
        }
    }

    fn build(&self, _view: &AnimationProbe, _ctx: &dyn BuildContext) -> impl IntoView {
        SizedBox::new(10.0, 10.0)
    }
}

fn animation_controller() -> AnimationController {
    AnimationController::new(Duration::from_secs(1), Arc::new(Scheduler::new()))
}

type AnimationProbeFixture = (
    AnimationProbe,
    Arc<Mutex<Option<bool>>>,
    Arc<AtomicUsize>,
    Arc<AtomicUsize>,
);

fn animation_probe(controller: &AnimationController) -> AnimationProbeFixture {
    let found_ambient = Arc::new(Mutex::new(None));
    let init_count = Arc::new(AtomicUsize::new(0));
    let dispose_count = Arc::new(AtomicUsize::new(0));
    (
        AnimationProbe {
            controller: controller.clone(),
            found_ambient: Arc::clone(&found_ambient),
            init_count: Arc::clone(&init_count),
            dispose_count: Arc::clone(&dispose_count),
        },
        found_ambient,
        init_count,
        dispose_count,
    )
}

#[test]
fn hidden_maintained_animation_is_muted_by_default() {
    let vsync = Vsync::new();
    let controller = animation_controller();
    let (probe, found_ambient, _init_count, _dispose_count) = animation_probe(&controller);
    let root = VsyncScope::new(
        vsync.clone(),
        Visibility::new(probe).maintain_state(true).visible(false),
    );
    let mut laid = lay_out_animated(root, loose(100.0), vsync);

    assert_eq!(*found_ambient.lock(), Some(true));
    controller.forward().expect("animation should start");
    laid.pump_for(FRAME);
    laid.pump_for(FRAME);
    assert_eq!(
        controller.value(),
        0.0,
        "a hidden maintained subtree should mute its ambient animation registry by default"
    );
    controller.dispose();
}

#[test]
fn visible_maintained_animation_advances_by_default() {
    let vsync = Vsync::new();
    let controller = animation_controller();
    let (probe, found_ambient, _init_count, _dispose_count) = animation_probe(&controller);
    let root = VsyncScope::new(vsync.clone(), Visibility::new(probe).maintain_state(true));
    let mut laid = lay_out_animated(root, loose(100.0), vsync);

    assert_eq!(*found_ambient.lock(), Some(true));
    controller.forward().expect("animation should start");
    laid.pump_for(FRAME);
    laid.pump_for(FRAME);
    assert!(controller.value() > 0.0);
    controller.dispose();
}

#[test]
fn hidden_maintained_animation_advances_when_requested() {
    let vsync = Vsync::new();
    let controller = animation_controller();
    let (probe, found_ambient, _init_count, _dispose_count) = animation_probe(&controller);
    let root = VsyncScope::new(
        vsync.clone(),
        Visibility::new(probe)
            .maintain_state(true)
            .maintain_animation(true)
            .visible(false),
    );
    let mut laid = lay_out_animated(root, loose(100.0), vsync);

    assert_eq!(*found_ambient.lock(), Some(true));
    controller.forward().expect("animation should start");
    laid.pump_for(FRAME);
    laid.pump_for(FRAME);
    assert!(controller.value() > 0.0);
    controller.dispose();
}

#[test]
fn disabled_ancestor_mutes_hidden_maintained_animation_even_when_requested() {
    let vsync = Vsync::new();
    let controller = animation_controller();
    let (probe, found_ambient, _init_count, _dispose_count) = animation_probe(&controller);
    let visibility = Visibility::new(probe)
        .maintain_state(true)
        .maintain_animation(true)
        .visible(false);
    let root = VsyncScope::new(vsync.clone(), TickerMode::new(visibility).enabled(false));
    let mut laid = lay_out_animated(root, loose(100.0), vsync);

    assert_eq!(*found_ambient.lock(), Some(true));
    controller.forward().expect("animation should start");
    laid.pump_for(FRAME);
    laid.pump_for(FRAME);
    assert_eq!(controller.value(), 0.0);
    controller.dispose();
}

#[test]
fn maintain_animation_accepts_either_valid_setter_order() {
    let first = lay_out(
        Visibility::new(SizedBox::new(10.0, 10.0))
            .maintain_animation(true)
            .maintain_state(true),
        loose(100.0),
    );
    let second = lay_out(
        Visibility::new(SizedBox::new(10.0, 10.0))
            .maintain_state(true)
            .maintain_animation(true),
        loose(100.0),
    );

    assert_eq!(first.size(first.root()), size(10.0, 10.0));
    assert_eq!(second.size(second.root()), size(10.0, 10.0));
}

#[test]
fn hidden_default_without_ambient_vsync_preserves_pass_through_behavior() {
    let controller = animation_controller();
    let (probe, found_ambient, _init_count, _dispose_count) = animation_probe(&controller);

    let _laid = lay_out(
        Visibility::new(probe).maintain_state(true).visible(false),
        loose(100.0),
    );

    assert_eq!(
        *found_ambient.lock(),
        Some(false),
        "without an ambient VsyncScope, Visibility's TickerMode passes the child through"
    );
    controller.dispose();
}

#[derive(Clone, StatelessView)]
struct AnimationPolicyHost {
    maintain_animation: Arc<AtomicBool>,
    probe: AnimationProbe,
}

impl StatelessView for AnimationPolicyHost {
    fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
        Visibility::new(self.probe.clone())
            .visible(false)
            .maintain_state(true)
            .maintain_animation(self.maintain_animation.load(Ordering::Relaxed))
    }
}

#[test]
fn changing_maintain_animation_from_true_to_false_remounts_the_child() {
    let vsync = Vsync::new();
    let controller = animation_controller();
    let (probe, _found_ambient, init_count, dispose_count) = animation_probe(&controller);
    let maintain_animation = Arc::new(AtomicBool::new(true));
    let host = AnimationPolicyHost {
        maintain_animation: Arc::clone(&maintain_animation),
        probe,
    };
    let root = VsyncScope::new(vsync.clone(), host);
    let mut laid = lay_out_animated(root, loose(100.0), vsync);

    assert_eq!(init_count.load(Ordering::Relaxed), 1);
    assert_eq!(dispose_count.load(Ordering::Relaxed), 0);

    maintain_animation.store(false, Ordering::Relaxed);
    laid.pump();

    assert_eq!(init_count.load(Ordering::Relaxed), 2);
    assert_eq!(dispose_count.load(Ordering::Relaxed), 1);
    controller.dispose();
}

#[derive(Clone, StatelessView)]
struct VisibilityToggleHost {
    visible: Arc<AtomicBool>,
    mounted: Arc<AtomicBool>,
    probe: AnimationProbe,
}

impl StatelessView for VisibilityToggleHost {
    fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
        let child: BoxedView = if self.mounted.load(Ordering::Relaxed) {
            Visibility::new(self.probe.clone())
                .visible(self.visible.load(Ordering::Relaxed))
                .maintain_state(true)
                .boxed()
        } else {
            SizedBox::shrink().boxed()
        };
        child
    }
}

#[test]
fn maintained_child_mutes_and_resumes_without_remounting_as_visibility_changes() {
    let vsync = Vsync::new();
    let controller = animation_controller();
    let (probe, found_ambient, init_count, dispose_count) = animation_probe(&controller);
    let visible = Arc::new(AtomicBool::new(true));
    let mounted = Arc::new(AtomicBool::new(true));
    let host = VisibilityToggleHost {
        visible: Arc::clone(&visible),
        mounted: Arc::clone(&mounted),
        probe,
    };
    let root = VsyncScope::new(vsync.clone(), host);
    let mut laid = lay_out_animated(root, loose(100.0), vsync);

    assert_eq!(*found_ambient.lock(), Some(true));
    assert_eq!(init_count.load(Ordering::Relaxed), 1);
    assert_eq!(dispose_count.load(Ordering::Relaxed), 0);

    controller.forward().expect("animation should start");
    laid.pump_for(FRAME);
    laid.pump_for(FRAME);
    let visible_value = controller.value();
    assert!(visible_value > 0.0, "the visible controller should advance");

    visible.store(false, Ordering::Relaxed);
    laid.pump();
    laid.pump_for(FRAME);
    laid.pump_for(FRAME);
    let hidden_value = controller.value();
    assert_eq!(
        hidden_value, visible_value,
        "the hidden controller should remain at its visible value"
    );
    assert_eq!(init_count.load(Ordering::Relaxed), 1);
    assert_eq!(dispose_count.load(Ordering::Relaxed), 0);

    visible.store(true, Ordering::Relaxed);
    laid.pump();
    laid.pump_for(FRAME);
    laid.pump_for(FRAME);
    assert!(
        controller.value() > hidden_value,
        "the controller should resume after becoming visible"
    );
    assert_eq!(init_count.load(Ordering::Relaxed), 1);
    assert_eq!(dispose_count.load(Ordering::Relaxed), 0);

    mounted.store(false, Ordering::Relaxed);
    laid.pump();
    assert_eq!(dispose_count.load(Ordering::Relaxed), 1);
    let unmounted_value = controller.value();
    laid.pump_for(FRAME);
    laid.pump_for(FRAME);
    assert_eq!(
        controller.value(),
        unmounted_value,
        "disposing the probe should unregister it from ambient Vsync"
    );
    controller.dispose();
}

#[cfg(debug_assertions)]
#[test]
fn invalid_maintain_animation_configuration_builds_one_error_child() {
    let view = Visibility::new(SizedBox::new(10.0, 10.0)).maintain_animation(true);
    let mut tree = ElementTree::new();
    let mut owner = BuildOwner::new();
    let root_id = tree.mount_root(&view, &mut owner.element_owner_mut());
    owner.schedule_build_for(root_id, 0);
    owner.build_scope(&mut tree);

    let child_ids: Vec<_> = tree
        .iter_nodes()
        .filter_map(|(id, node)| (node.parent() == Some(root_id)).then_some(id))
        .collect();
    assert_eq!(child_ids.len(), 1);
    assert_eq!(
        tree.get(child_ids[0])
            .expect("the substituted error child should exist")
            .element()
            .view_type_id(),
        TypeId::of::<ErrorView>()
    );
}

#[test]
fn default_visible_shows_the_child_directly_with_no_offstage_wrapper() {
    let laid = lay_out(Visibility::new(SizedBox::new(30.0, 20.0)), loose(1000.0));

    assert!(
        laid.find_all_by_render_type("RenderOffstage").is_empty(),
        "the default (maintain_state = false) path must not wrap the child \
         in Offstage at all",
    );
    assert_eq!(laid.size(laid.root()), size(30.0, 20.0));
}

#[test]
fn hidden_without_maintain_state_shows_the_default_replacement() {
    let laid = lay_out(
        Visibility::new(SizedBox::new(30.0, 20.0)).visible(false),
        loose(1000.0),
    );

    // Default replacement is SizedBox::shrink() -- the real 30x20 child must
    // be entirely absent, replaced by a zero-size box.
    assert_eq!(laid.size(laid.root()), size(0.0, 0.0));
}

#[test]
fn hidden_without_maintain_state_uses_a_custom_replacement() {
    let laid = lay_out(
        Visibility::new(SizedBox::new(30.0, 20.0))
            .visible(false)
            .replacement(SizedBox::new(5.0, 5.0)),
        loose(1000.0),
    );

    assert_eq!(laid.size(laid.root()), size(5.0, 5.0));
}

#[test]
fn maintain_state_true_and_visible_wraps_the_child_in_a_non_offstage_offstage() {
    let laid = lay_out(
        Visibility::new(SizedBox::new(30.0, 20.0)).maintain_state(true),
        loose(1000.0),
    );

    let offstage_id = laid.find_by_render_type("RenderOffstage");
    assert_eq!(
        laid.size(offstage_id),
        size(30.0, 20.0),
        "visible = true must report the child's real size through Offstage \
         (transparent-proxy branch, offstage = false)",
    );
}

#[test]
fn maintain_state_true_and_hidden_wraps_the_child_in_an_offstage_offstage() {
    let laid = lay_out(
        Visibility::new(SizedBox::new(30.0, 20.0))
            .maintain_state(true)
            .visible(false),
        loose(1000.0),
    );

    let offstage_id = laid.find_by_render_type("RenderOffstage");
    // `RenderOffstage` takes `constraints.smallest()` when offstage (Flutter's
    // `sizedByParent => offstage`). Under `loose(1000)` that is zero. The child
    // is laid out at its full size regardless — asserted in the test below.
    assert_eq!(
        laid.size(offstage_id),
        size(0.0, 0.0),
        "visible = false with maintain_state must take constraints.smallest() \
         while keeping the child attached (state preserved, not removed)",
    );
    // The child render node must still be present in the tree (state kept
    // alive), unlike the maintain_state = false replacement path.
    assert_eq!(laid.render_node_count(), 2, "RenderOffstage + the child");
}

/// The widget-level consequence of `RenderOffstage`'s layout contract: a
/// hidden-but-maintained child is laid out at its **full size**, not collapsed to zero.
///
/// Flutter's `RenderOffstage.performLayout` does `child?.layout(constraints)`
/// with the real constraints (`proxy_box.dart:3919-3925`); only the `Offstage`
/// box itself shrinks to `constraints.smallest`. This is what makes
/// `ModalRoute.offstage` able to measure a route at its final geometry.
///
/// Red-check: lay the child out at `BoxConstraints::tight(Size::ZERO)` in
/// `RenderOffstage::perform_layout`; the child measures 0×0.
#[test]
fn maintain_state_true_and_hidden_lays_the_child_out_at_full_size() {
    let laid = lay_out(
        Visibility::new(SizedBox::new(30.0, 20.0))
            .maintain_state(true)
            .visible(false),
        loose(1000.0),
    );

    let offstage_id = laid.find_by_render_type("RenderOffstage");
    assert_eq!(
        laid.size(offstage_id),
        size(0.0, 0.0),
        "the Offstage box takes constraints.smallest() — zero, under loose"
    );
    assert_eq!(
        laid.size(laid.only_child(offstage_id)),
        size(30.0, 20.0),
        "but the hidden child reaches its real geometry"
    );
}
