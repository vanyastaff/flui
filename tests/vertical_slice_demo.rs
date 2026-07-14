//! Acceptance test for the vertical-slice demo.
//!
//! `#[path]`-includes the exact tree `examples/vertical_slice_demo/main.rs`
//! runs (not a duplicate) and mounts it through `flui_binding::HeadlessBinding`'s
//! public surface, then drives it the way an app author's fingers would: tap
//! the "+" button, change the list's scroll position, tap the animated box —
//! asserting on the resulting render tree, not merely "no panic".
//!
//! `flui-widgets`' `tests/common` harness lives in a different crate (a
//! private integration-test module, unreachable from here): this test lives
//! in the root crate, so it re-bootstraps a headless tree from `flui-view` /
//! `flui-rendering` / `flui-binding`'s public API only, mirroring the
//! sequence `HeadlessBinding`'s own docs describe (mount root -> attach
//! `PipelineOwner` -> set root constraints -> run one frame -> `bind_tree`).
//!
//! Honesty notes (Definition of Done):
//! - The counter and animated-box assertions are fully gesture-driven
//!   (synthetic pointer taps through the mounted tree) — the same path a
//!   real user would exercise.
//! - `ListView` (the widget this demo composes) has no drag-to-scroll
//!   gesture wired in `flui-widgets` yet — it is a `Viewport` over a sliver
//!   with a purely programmatic `offset`. Dragging
//!   through it fires no scroll. The scroll assertion therefore falls back
//!   to the documented alternative: mutate `DemoRoot::scroll_offset`
//!   directly and force a rebuild, then assert the resulting layout
//!   (paint offset) actually moved — still a real behavioral assertion on
//!   the render tree, not a tautology, just not drag-driven.

#[path = "../examples/vertical_slice_demo/tree.rs"]
mod tree;

use std::sync::Arc;
use std::time::Duration;

use flui_binding::HeadlessBinding;
use flui_foundation::{ElementId, RenderId};
use flui_interaction::events::{PointerType, make_down_event, make_up_event};
use flui_rendering::constraints::BoxConstraints;
use flui_rendering::hit_testing::HitTestResult;
use flui_rendering::pipeline::PipelineOwner;
use flui_rendering::testing::inspect;
use flui_types::geometry::px;
use flui_types::{Offset, Size};
use flui_view::{BuildOwner, ElementTree};
use flui_widgets::VsyncScope;
use parking_lot::RwLock;

/// Root constraints the demo is mounted under: wide/tall enough that the
/// counter row, the [`tree::LIST_BOX_HEIGHT`]-tall list box, and the
/// animated box (up to [`tree::EXPANDED_HEIGHT`] tall) all coexist in the
/// `Column` without overflow.
fn root_constraints() -> BoxConstraints {
    BoxConstraints::tight(Size::new(px(480.0), px(720.0)))
}

/// Everything the test needs to drive and inspect the mounted demo tree.
struct MountedDemo {
    binding: HeadlessBinding,
    /// The `Rc<Cell<_>>` handles from the exact `DemoRoot` that was mounted
    /// (cloned before it was wrapped and moved into the tree), so the test
    /// can read/mutate the same cells `DemoRootState::build` reads.
    handles: tree::DemoRoot,
    /// The tree's mounted root element (the outermost `VsyncScope`).
    root_element: ElementId,
    pipeline_owner: Arc<RwLock<PipelineOwner>>,
}

impl MountedDemo {
    /// Mount `tree::demo_root()` under a `VsyncScope` over `binding`'s own
    /// registry (so `pump_frame` ticks the animated box's controller), run
    /// the bootstrap frame, then hand the owners to `binding`.
    ///
    /// The example wraps `DemoRoot` in a thin `DemoApp` (`StatelessView`)
    /// adapter only because `flui_app::run_app` requires a stateless root
    /// (see `tree.rs`'s `DemoApp` doc); `DemoApp::build` returns `DemoRoot`
    /// unchanged, so mounting `DemoRoot` directly here is the identical
    /// tree, minus that pass-through wrapper.
    fn mount() -> Self {
        let mut binding = HeadlessBinding::new();

        let root_view = tree::demo_root();
        let handles = root_view.clone();

        let mut build_owner = BuildOwner::new();
        let mut tree = ElementTree::new();
        let pipeline_owner = Arc::new(RwLock::new(PipelineOwner::new()));

        // Install the async-driver / post-frame / interaction-dispatch
        // capabilities on this binding's owner BEFORE the mount build pass —
        // `DemoRootState::init_state` calls `ctx.rebuild_handle()`, which
        // needs the owner already wired to `binding`'s scheduler.
        binding.install_build_capabilities(&mut build_owner);

        let scoped_root = VsyncScope::new(binding.vsync().clone(), root_view);

        let root_element = binding.enter_owner_scope(|| {
            let root_element = tree.mount_root_with_pipeline_owner(
                &scoped_root,
                Some(Arc::clone(&pipeline_owner)),
                &mut build_owner.element_owner_mut(),
            );
            build_owner.schedule_build_for(root_element, 0);
            build_owner.build_scope(&mut tree);
            root_element
        });

        let root_render_id = {
            let owner = pipeline_owner.read();
            let render_tree = owner.render_tree();
            let mut roots = render_tree
                .iter()
                .map(|(id, _)| id)
                .filter(|id| render_tree.parent(*id).is_none());
            let root = roots
                .next()
                .expect("the mounted demo tree should have a render root");
            assert!(
                roots.next().is_none(),
                "expected exactly one render-tree root after mount"
            );
            root
        };

        {
            let mut guard = pipeline_owner.write();
            guard.set_root_id(Some(root_render_id));
            guard.set_root_constraints(Some(root_constraints()));
        }

        binding.enter_owner_scope(|| {
            build_owner
                .run_frame_with_layout_builders(&mut tree, &pipeline_owner)
                .expect("bootstrap frame over the demo tree should succeed");
        });

        binding.bind_tree(build_owner, tree, Arc::clone(&pipeline_owner));

        Self {
            binding,
            handles,
            root_element,
            pipeline_owner,
        }
    }

    /// Drive one deterministic frame.
    fn pump(&mut self, dt: Duration) {
        self.binding.pump_frame(dt);
    }

    /// Force the root element to rebuild on the next [`pump`](Self::pump) —
    /// the headless equivalent of an external `setState`. Used only for the
    /// list's scroll offset, which (unlike the counter/animated box) has no
    /// gesture path scheduling its own rebuild.
    fn mark_root_dirty(&mut self) {
        if let Some(node) = self.binding.tree_mut().get_mut(self.root_element) {
            node.element_mut().mark_needs_build();
        }
        self.binding
            .build_owner_mut()
            .schedule_build_for(self.root_element, 0);
    }

    /// Hit-test at root-local `(x, y)` and dispatch a synthetic pointer-down.
    fn tap_down(&self, x: f32, y: f32) {
        self.dispatch_at(make_down_event(offset(x, y), PointerType::Mouse), x, y);
    }

    /// Hit-test at root-local `(x, y)` and dispatch a synthetic pointer-up —
    /// paired with [`tap_down`](Self::tap_down) at the same position, this
    /// completes a tap (`TapGestureRecognizer` fires `on_tap`).
    fn tap_up(&self, x: f32, y: f32) {
        self.dispatch_at(make_up_event(offset(x, y), PointerType::Mouse), x, y);
    }

    fn dispatch_at(&self, event: flui_interaction::PointerEvent, x: f32, y: f32) {
        let position = offset(x, y);
        let owner = self.pipeline_owner.read();
        let mut result = HitTestResult::new();
        owner.hit_test(position, &mut result);
        self.binding.enter_owner_scope(|| result.dispatch(&event));
    }

    /// A full tap (down + up) at `(x, y)`.
    fn tap(&self, x: f32, y: f32) {
        self.tap_down(x, y);
        self.tap_up(x, y);
    }

    fn find_all_by_render_type(&self, render_type_name: &str) -> Vec<RenderId> {
        let owner = self.pipeline_owner.read();
        owner
            .render_tree()
            .iter()
            .filter_map(|(id, _node)| {
                let diagnostics = owner.debug_node_diagnostics(id)?;
                (diagnostics.name() == Some(render_type_name)).then_some(id)
            })
            .collect()
    }

    /// The unique `RenderParagraph` node whose plain-text content is `text`.
    fn find_text(&self, text: &str) -> Option<RenderId> {
        let owner = self.pipeline_owner.read();
        let mut found = None;
        for (id, _node) in owner.render_tree().iter() {
            let Some(diagnostics) = owner.debug_node_diagnostics(id) else {
                continue;
            };
            if diagnostics.name() != Some("RenderParagraph") {
                continue;
            }
            if diagnostics.get_property("text") == Some(text) {
                assert!(
                    found.is_none(),
                    "multiple RenderParagraph nodes contain {text:?}"
                );
                found = Some(id);
            }
        }
        found
    }

    /// The animated box's own `RenderConstrainedBox`.
    ///
    /// The demo tree mounts several `RenderConstrainedBox` nodes (the "+"
    /// button's spacer `SizedBox`, the list's fixed-height wrapper, the
    /// animated box's zero-size filler child, and the animated box's own
    /// width/height constraint), so type-name lookup alone is ambiguous.
    /// The animated box is the only one whose *committed width and height
    /// are simultaneously* within `[COLLAPSED, EXPANDED]` on both axes —
    /// every other candidate fails on at least one axis by construction
    /// (`tree.rs`'s constants keep the ranges disjoint).
    ///
    /// # Panics
    /// Panics when zero or more than one candidate matches.
    fn animated_box_render_id(&self) -> RenderId {
        let owner = self.pipeline_owner.read();
        let width_range = tree::COLLAPSED_WIDTH.min(tree::EXPANDED_WIDTH) - 1.0
            ..=tree::COLLAPSED_WIDTH.max(tree::EXPANDED_WIDTH) + 1.0;
        let height_range = tree::COLLAPSED_HEIGHT.min(tree::EXPANDED_HEIGHT) - 1.0
            ..=tree::COLLAPSED_HEIGHT.max(tree::EXPANDED_HEIGHT) + 1.0;
        let matches: Vec<RenderId> = self
            .find_all_by_render_type("RenderConstrainedBox")
            .into_iter()
            .filter(|&id| {
                inspect::box_geometry(&owner, id).is_some_and(|size| {
                    width_range.contains(&size.width.get())
                        && height_range.contains(&size.height.get())
                })
            })
            .collect();
        match matches.as_slice() {
            [id] => *id,
            [] => panic!("no RenderConstrainedBox falls within the animated box's size range"),
            _ => panic!(
                "{} RenderConstrainedBox nodes fall within the animated box's size range; \
                 expected exactly one",
                matches.len()
            ),
        }
    }

    /// The screen-space (root-local) top-left of `id`, by summing paint
    /// offsets up the render-tree ancestry — every node between the root and
    /// `id` in this tree only translates (no scale/rotation), so a plain sum
    /// recovers the absolute position.
    fn absolute_position(&self, id: RenderId) -> Offset {
        let owner = self.pipeline_owner.read();
        let render_tree = owner.render_tree();
        let mut x = 0.0f32;
        let mut y = 0.0f32;
        let mut current = id;
        loop {
            if let Some(offset) = inspect::render_offset(&owner, current) {
                x += offset.dx.get();
                y += offset.dy.get();
            }
            match render_tree.parent(current) {
                Some(parent) => current = parent,
                None => break,
            }
        }
        offset(x, y)
    }
}

fn offset(x: f32, y: f32) -> Offset {
    Offset::new(px(x), px(y))
}

// ============================================================================
// (a) tap the "+" button -> the rendered counter text changes
// ============================================================================

#[test]
fn tapping_the_plus_button_updates_the_rendered_counter_text() {
    let mut demo = MountedDemo::mount();

    assert!(
        demo.find_text("Count: 0").is_some(),
        "the counter must render its initial value"
    );

    // Locate the "+" glyph and tap its rendered position.
    let plus = demo
        .find_text("+")
        .expect("the '+' button's Text must be in the render tree");
    let tap_at = demo.absolute_position(plus);
    demo.tap(tap_at.dx.get() + 1.0, tap_at.dy.get() + 1.0);

    // The tap's on_tap handler scheduled a rebuild via RebuildHandle; the
    // next pump drains it.
    demo.pump(Duration::ZERO);

    assert!(
        demo.find_text("Count: 0").is_none(),
        "the stale 'Count: 0' text must be gone after the tap rebuilds the counter"
    );
    assert!(
        demo.find_text("Count: 1").is_some(),
        "tapping '+' once must rebuild the counter text to 'Count: 1'"
    );

    // A second tap keeps incrementing — proves the element (and its
    // RebuildHandle) survives across rebuilds rather than being torn down.
    demo.tap(tap_at.dx.get() + 1.0, tap_at.dy.get() + 1.0);
    demo.pump(Duration::ZERO);
    assert!(
        demo.find_text("Count: 2").is_some(),
        "a second tap must advance the counter to 'Count: 2'"
    );
}

// ============================================================================
// (b) scrolling changes visible content — programmatic-offset fallback
// ============================================================================

#[test]
fn changing_the_list_scroll_offset_moves_its_items() {
    let mut demo = MountedDemo::mount();

    let item0 = demo
        .find_text("Item 0")
        .expect("the static list must render its first item");
    let offset_before = demo.absolute_position(item0);

    // `ListView` (this composition) has no drag-to-scroll gesture wired in
    // flui-widgets yet — it is a `Viewport` over a sliver with a purely
    // programmatic `offset`, no `Scrollable` ancestor. A pointer drag
    // through the list area would therefore fire no scroll at all, so this
    // asserts the documented fallback instead: mutate the shared offset cell
    // directly and force a rebuild, then check the resulting layout moved.
    let scroll_delta = 3.0 * tree::LIST_ITEM_EXTENT;
    demo.handles.scroll_offset.set(scroll_delta);
    demo.mark_root_dirty();
    demo.pump(Duration::ZERO);

    let offset_after = demo.absolute_position(item0);
    assert!(
        (offset_after.dy.get() - offset_before.dy.get()).abs() > 1.0,
        "changing the list's scroll offset must move its items' paint \
         position: before={offset_before:?}, after={offset_after:?}"
    );
    // A `Viewport` translates its sliver content by `-offset` along the
    // scroll axis: increasing the offset must move content UP (a smaller
    // or more negative `dy`), matching the standard scroll convention.
    assert!(
        offset_after.dy.get() < offset_before.dy.get(),
        "increasing the scroll offset must move item 0 upward: \
         before={offset_before:?}, after={offset_after:?}"
    );
}

// ============================================================================
// (c) tap the animated box -> the animated property interpolates, then
//     settles at the target
// ============================================================================

#[test]
fn tapping_the_animated_box_interpolates_width_to_the_expanded_target() {
    let mut demo = MountedDemo::mount();

    let box_id = demo.animated_box_render_id();
    let width_at_rest = inspect::box_geometry(&demo.pipeline_owner.read(), box_id)
        .expect("the animated box must have box geometry after the bootstrap frame")
        .width
        .get();
    assert!(
        (width_at_rest - tree::COLLAPSED_WIDTH).abs() < 0.5,
        "the animated box starts at its collapsed width, got {width_at_rest}"
    );

    // Tap the box (Opaque hit-test behavior, so any point inside its bounds
    // works) to toggle `expanded` and retarget the controller.
    let tap_at = demo.absolute_position(box_id);
    demo.tap(tap_at.dx.get() + 2.0, tap_at.dy.get() + 2.0);
    demo.pump(Duration::ZERO); // the detection frame: rebuild + retarget, t = 0

    let box_id = demo.animated_box_render_id();
    let width_after_retarget = inspect::box_geometry(&demo.pipeline_owner.read(), box_id)
        .expect("geometry after retarget")
        .width
        .get();
    assert!(
        (width_after_retarget - tree::COLLAPSED_WIDTH).abs() < 0.5,
        "the detection frame (t=0) must still show the collapsed width, got {width_after_retarget}"
    );

    // Pump several ~16ms frames through the run (240ms / 16ms = 15 frames)
    // and record width samples; the run must climb monotonically from the
    // collapsed to the expanded width, passing strictly through it midway.
    let frame = Duration::from_millis(16);
    let mut samples = Vec::new();
    for _ in 0..16 {
        demo.pump(frame);
        let id = demo.animated_box_render_id();
        let width = inspect::box_geometry(&demo.pipeline_owner.read(), id)
            .expect("geometry mid-flight")
            .width
            .get();
        samples.push(width);
    }

    for pair in samples.windows(2) {
        assert!(
            pair[1] >= pair[0] - 0.5,
            "the animated width must not regress across frames: {samples:?}"
        );
    }
    let midpoint = samples[3]; // ~64ms into a 240ms run
    assert!(
        midpoint > tree::COLLAPSED_WIDTH + 1.0 && midpoint < tree::EXPANDED_WIDTH - 1.0,
        "an intermediate frame must show a width strictly between the collapsed \
         ({}) and expanded ({}) targets, got {midpoint}: {samples:?}",
        tree::COLLAPSED_WIDTH,
        tree::EXPANDED_WIDTH,
    );
    let settled = *samples.last().expect("at least one sample");
    assert!(
        (settled - tree::EXPANDED_WIDTH).abs() < 0.5,
        "after the 240ms run has fully elapsed the box must equal its \
         expanded target ({}), got {settled}",
        tree::EXPANDED_WIDTH,
    );
}

// ============================================================================
// `tree.rs` sanity — both `#[path]` consumers reference the same symbols
// ============================================================================

/// `DemoApp` (the thin `StatelessView` `flui_app::run_app` entry point) is
/// exercised at runtime only by `examples/vertical_slice_demo/main.rs`, not
/// this headless test — the acceptance tests above mount `DemoRoot` directly
/// (see `MountedDemo::mount`'s doc). Referencing it here keeps both
/// `#[path]` consumers of `tree.rs` compiling the same symbol set, so a
/// signature change that breaks the example's entry point fails `cargo test`
/// too, not only `cargo build --example`.
#[test]
fn demo_app_entry_point_constructs() {
    let _ = tree::DemoApp;
}
