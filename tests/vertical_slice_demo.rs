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
//! - All three acceptance assertions (counter, drag-to-scroll, animated box)
//!   are fully gesture-driven (synthetic pointer down/move/up through the
//!   mounted tree) — the same path a real user's fingers would exercise.
//! - The list's drag-to-scroll is demo-local wiring: a `GestureDetector`
//!   feeding a `ScrollController` directly, NOT the `Scrollable` widget.
//!   `Scrollable` hardwires a `SingleChildScrollView` with no offset
//!   feed-through (`scrollable.rs`), and nesting `ListView` inside it would
//!   produce a degenerate viewport — the framework-level fix (a `Scrollable`
//!   that accepts an arbitrary scrollable child) is out of scope here; see
//!   `tree.rs`'s module doc.
//! - Drag-only: there is no fling/ballistic simulation. `on_pan_end`'s
//!   release velocity is intentionally unused — hand-rolling ballistics in
//!   the demo was ruled out; a real fling awaits the same framework-level
//!   item.

#[path = "../examples/vertical_slice_demo/tree.rs"]
mod tree;

use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant};

use flui_binding::HeadlessBinding;
use flui_foundation::RenderId;
use flui_interaction::events::{PointerType, make_down_event, make_move_event, make_up_event};
use flui_rendering::constraints::BoxConstraints;
use flui_rendering::hit_testing::HitTestResult;
use flui_rendering::pipeline::PipelineOwner;
use flui_rendering::testing::inspect;
use flui_types::geometry::px;
use flui_types::{Offset, Size};
use flui_view::{BuildOwner, ElementTree};
use flui_widgets::{FocusRoot, GestureArenaScope, VsyncScope};
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
    pipeline_owner: Arc<RwLock<PipelineOwner>>,
    /// Clone of the mounted [`tree::DemoRoot`]'s `home_create_count` — how many
    /// times `DemoHomeState::create_state` has run. See that field's doc for
    /// why this, and not a display assertion, is what proves state survival.
    home_create_count: Rc<Cell<u32>>,
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
        let home_create_count = Rc::clone(&root_view.home_create_count);

        let mut build_owner = BuildOwner::new();
        let mut tree = ElementTree::new();
        let pipeline_owner = Arc::new(RwLock::new(PipelineOwner::new()));

        // Install the async-driver / post-frame / interaction-dispatch
        // capabilities on this binding's owner BEFORE the mount build pass —
        // `DemoRootState::init_state` calls `ctx.rebuild_handle()`, which
        // needs the owner already wired to `binding`'s scheduler.
        binding.install_build_capabilities(&mut build_owner);

        let focused_root = FocusRoot::new(root_view);
        let animated_root = VsyncScope::new(binding.vsync().clone(), focused_root);
        let scoped_root = GestureArenaScope::new(binding.arena().clone(), animated_root);

        binding.enter_owner_scope(|| {
            let root_element = tree.mount_root_with_pipeline_owner(
                &scoped_root,
                Some(Arc::clone(&pipeline_owner)),
                &mut build_owner.element_owner_mut(),
            );
            build_owner.schedule_build_for(root_element, 0, flui_view::RebuildReason::InitialMount);
            build_owner.build_scope(&mut tree);
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
            pipeline_owner,
            home_create_count,
        }
    }

    /// How many times `DemoHomeState::create_state` has run so far.
    fn home_create_count(&self) -> u32 {
        self.home_create_count.get()
    }

    /// Drive one deterministic frame.
    fn pump(&mut self, dt: Duration) {
        self.binding.pump_frame(dt);
    }

    /// Hit-test at root-local `(x, y)` and dispatch a synthetic pointer-down.
    fn tap_down(&self, x: f32, y: f32) {
        self.dispatch_pointer(make_down_event(offset(x, y), PointerType::Mouse));
    }

    /// Hit-test at root-local `(x, y)` and dispatch a synthetic pointer-up —
    /// paired with [`tap_down`](Self::tap_down) at the same position, this
    /// completes a tap (`TapGestureRecognizer` fires `on_tap`).
    fn tap_up(&self, x: f32, y: f32) {
        self.dispatch_pointer(make_up_event(offset(x, y), PointerType::Mouse));
    }

    fn hit_test(&self, position: Offset) -> HitTestResult {
        let owner = self.pipeline_owner.read();
        let mut result = HitTestResult::new();
        owner.hit_test(position, &mut result);
        result
    }

    fn dispatch_pointer(&self, event: flui_interaction::PointerEvent) {
        self.binding
            .dispatch_pointer(&event, |position| self.hit_test(position));
    }

    /// A full tap (down + up) at `(x, y)`.
    fn tap(&self, x: f32, y: f32) {
        self.tap_down(x, y);
        self.tap_up(x, y);
    }

    /// Hit-test at root-local `(x, y)` and dispatch a synthetic pointer-down,
    /// advancing the gesture clock first so the drag recognizer's first
    /// velocity-tracker sample gets a fresh timestamp (see
    /// [`advance_gesture_clock`]). Distinct from [`tap_down`](Self::tap_down):
    /// only drag sequences need the clock advance, and `tap_down` is shared by
    /// unrelated tests this change must not perturb.
    fn drag_down(&self, x: f32, y: f32) {
        advance_gesture_clock();
        self.dispatch_pointer(make_down_event(offset(x, y), PointerType::Mouse));
    }

    /// Hit-test at root-local `(x, y)` and dispatch a synthetic pointer-move,
    /// advancing the gesture clock first (see [`advance_gesture_clock`]).
    fn drag_move(&self, x: f32, y: f32) {
        advance_gesture_clock();
        self.dispatch_pointer(make_move_event(offset(x, y), PointerType::Mouse));
    }

    /// Hit-test at root-local `(x, y)` and dispatch a synthetic pointer-up —
    /// pairs with [`drag_down`](Self::drag_down)/[`drag_move`](Self::drag_move)
    /// to complete a drag gesture.
    fn drag_up(&self, x: f32, y: f32) {
        self.dispatch_pointer(make_up_event(offset(x, y), PointerType::Mouse));
    }

    /// Compares base names (before any `<...>`) on both sides — a
    /// diagnostics node's own name keeps full generic fidelity (e.g.
    /// `"RenderViewport<ScrollPosition>"`), but a caller querying "by render
    /// type" wants the base name regardless of which generic argument a
    /// render object happens to be monomorphized over. See the identical
    /// helper (and its rationale) in `flui-widgets/tests/common/mod.rs`,
    /// which this test cannot import (its own `common` module lives in a
    /// different crate — see the module doc's re-bootstrapping note).
    fn find_all_by_render_type(&self, render_type_name: &str) -> Vec<RenderId> {
        let owner = self.pipeline_owner.read();
        let queried = base_type_name(render_type_name);
        owner
            .render_tree()
            .iter()
            .filter_map(|(id, _node)| {
                let diagnostics = owner.debug_node_diagnostics(id)?;
                (diagnostics.name().map(base_type_name) == Some(queried)).then_some(id)
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

    /// The list's fixed-height `SizedBox` wrapper.
    ///
    /// Same disambiguation problem as [`animated_box_render_id`]'s doc
    /// explains: several `RenderConstrainedBox` nodes exist in this tree.
    /// This one is the unique node whose committed height equals
    /// [`tree::LIST_BOX_HEIGHT`] (200px) — distinct by construction from the
    /// counter spacer and the animated box's collapsed/expanded height range
    /// (64/140px).
    ///
    /// # Panics
    /// Panics when zero or more than one candidate matches.
    fn list_box_render_id(&self) -> RenderId {
        let owner = self.pipeline_owner.read();
        let matches: Vec<RenderId> = self
            .find_all_by_render_type("RenderConstrainedBox")
            .into_iter()
            .filter(|&id| {
                inspect::box_geometry(&owner, id)
                    .is_some_and(|size| (size.height.get() - tree::LIST_BOX_HEIGHT).abs() < 1.0)
            })
            .collect();
        match matches.as_slice() {
            [id] => *id,
            [] => panic!(
                "no RenderConstrainedBox matches the list box height ({})",
                tree::LIST_BOX_HEIGHT
            ),
            _ => panic!(
                "{} RenderConstrainedBox nodes match the list box height; expected exactly one",
                matches.len()
            ),
        }
    }

    /// The list box's root-local center — a safe drag anchor: enough headroom
    /// above and below to cross the slop and keep moving without the pointer
    /// leaving the box (which would hit-test a different render path on the
    /// next move).
    fn list_box_center(&self) -> (f32, f32) {
        let list_box = self.list_box_render_id();
        let size = inspect::box_geometry(&self.pipeline_owner.read(), list_box)
            .expect("the list box must have box geometry after the bootstrap frame");
        let top_left = self.absolute_position(list_box);
        (
            top_left.dx.get() + size.width.get() / 2.0,
            top_left.dy.get() + size.height.get() / 2.0,
        )
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

/// The part of `type_name` before its first `<`, if any — the base name
/// ignoring generic parameters ("RenderViewport<ScrollPosition>" ->
/// "RenderViewport"; a non-generic name passes through unchanged).
fn base_type_name(type_name: &str) -> &str {
    type_name.split('<').next().unwrap_or(type_name)
}

/// Spin until `Instant::now()` returns a value strictly greater than the one
/// returned by the immediately preceding call.
///
/// Mirrors `flui-widgets/tests/common/mod.rs`'s
/// `LaidOutScoped::advance_gesture_clock` (unreachable from this crate — that
/// harness lives in a private integration-test module of a different crate,
/// same reason `MountedDemo` re-bootstraps its own mount sequence instead of
/// reusing it). `DragGestureRecognizer::handle_move` timestamps every
/// velocity-tracker sample with `Instant::now()`; two dispatches landing in
/// the same OS timer tick make the least-squares velocity fit singular
/// (NaN). Calling this before each down/move dispatch that should count
/// toward velocity guarantees consecutive samples get strictly increasing
/// timestamps.
fn advance_gesture_clock() {
    let t0 = Instant::now();
    while Instant::now() == t0 {
        std::hint::spin_loop();
    }
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
// (b) dragging inside the list box scrolls it — real gesture-driven, no
//     programmatic fallback
// ============================================================================

/// Real per-move drag threshold for `GestureDetector`'s pan recognizer
/// (`DragAxis::Free`).
///
/// `GestureDetector` constructs its `DragGestureRecognizer` once in
/// `init_state` with `GestureSettings::default()`
/// (`flui-interaction/src/recognizers/drag.rs`) and never adapts it to the
/// dispatched pointer's device kind — so the operative slop is the *touch*
/// default (`DEFAULT_PAN_SLOP` = 18px), not the mouse default, even though
/// these events dispatch as `PointerType::Mouse`. This matches the identical
/// "50 px > 18 px" comment on `flui-widgets/tests/scroll.rs`'s
/// `scrollable_drag_up_increases_scroll_offset`, which exercises the same
/// recognizer through `Scrollable`.
const DRAG_SLOP: f32 = 18.0;

#[test]
fn dragging_inside_the_list_box_scrolls_its_items() {
    let mut demo = MountedDemo::mount();

    let item0 = demo
        .find_text("Item 0")
        .expect("the static list must render its first item");
    let offset_before = demo.absolute_position(item0);
    let (anchor_x, anchor_y) = demo.list_box_center();

    // This hit path has one arena member: the list's pan recognizer. Closing
    // the arena after Down therefore schedules that sole member as the
    // deferred default winner, and the binding drains the resolution before
    // returning from the Down transaction. The drag is already Started when
    // the first move arrives, so every move contributes its full delta.
    //
    // This is Flutter's `GestureArenaManager.close` +
    // `DragGestureRecognizer.onlyAcceptDragOnThreshold == false` behavior.
    // A competing tap recognizer would keep the arena unresolved until the
    // drag crosses slop and would instead re-anchor `DragStartBehavior::Start`
    // at the crossing position.
    const SLOP_CROSSING_DELTA: f32 = DRAG_SLOP + 7.0; // 25.0, safely > 18.0
    const UPDATE_DELTA_1: f32 = 20.0;
    const UPDATE_DELTA_2: f32 = 25.0;
    let expected_scroll_delta = SLOP_CROSSING_DELTA + UPDATE_DELTA_1 + UPDATE_DELTA_2;

    demo.drag_down(anchor_x, anchor_y);
    demo.drag_move(anchor_x, anchor_y - SLOP_CROSSING_DELTA);
    demo.drag_move(anchor_x, anchor_y - SLOP_CROSSING_DELTA - UPDATE_DELTA_1);
    demo.drag_move(
        anchor_x,
        anchor_y - SLOP_CROSSING_DELTA - UPDATE_DELTA_1 - UPDATE_DELTA_2,
    );
    demo.drag_up(
        anchor_x,
        anchor_y - SLOP_CROSSING_DELTA - UPDATE_DELTA_1 - UPDATE_DELTA_2,
    );
    demo.pump(Duration::ZERO);

    let offset_after = demo.absolute_position(item0);
    // A `Viewport` translates its sliver content by `-offset` along the
    // scroll axis, so an increasing offset must move content UP (a smaller
    // `dy`) — the standard scroll convention, matching `Scrollable`'s own
    // pan-update wiring in `scrollable.rs`.
    let moved_up_by = offset_before.dy.get() - offset_after.dy.get();

    assert!(
        (moved_up_by - expected_scroll_delta).abs() < 1.0,
        "dragging a lone recognizer up by {expected_scroll_delta}px must move item 0's paint \
         position up by the same amount: \
         before={offset_before:?}, after={offset_after:?}, moved_up_by={moved_up_by}"
    );
}

/// At the top of the list (offset 0, the fresh-mount default) a downward drag
/// proposes a *negative* pixel value (`pixels() - delta.dy` with `delta.dy >
/// 0`) — `ScrollController::jump_to`'s lower clamp must hold it at 0 through
/// the real gesture path, not just in `scroll_controller.rs`'s unit tests.
#[test]
fn dragging_down_at_the_top_of_the_list_does_not_scroll_past_zero() {
    let mut demo = MountedDemo::mount();

    let item0 = demo
        .find_text("Item 0")
        .expect("the static list must render its first item");
    let offset_before = demo.absolute_position(item0);
    let (anchor_x, anchor_y) = demo.list_box_center();

    // Post-slop deltas toward positive dy (finger moving down) — the mirror
    // image of the upward drag above.
    const SLOP_CROSSING_DELTA: f32 = DRAG_SLOP + 7.0; // 25.0, safely > 18.0
    const UPDATE_DELTA: f32 = 20.0;

    demo.drag_down(anchor_x, anchor_y);
    demo.drag_move(anchor_x, anchor_y + SLOP_CROSSING_DELTA);
    demo.drag_move(anchor_x, anchor_y + SLOP_CROSSING_DELTA + UPDATE_DELTA);
    demo.drag_up(anchor_x, anchor_y + SLOP_CROSSING_DELTA + UPDATE_DELTA);
    demo.pump(Duration::ZERO);

    let offset_after = demo.absolute_position(item0);
    assert!(
        (offset_after.dy.get() - offset_before.dy.get()).abs() < 1.0,
        "dragging down at the top of the list (offset already 0) must not move item 0 at all — \
         jump_to's lower clamp must hold: before={offset_before:?}, after={offset_after:?}"
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
// (d) tap the details button -> a navigated route pushes over the demo,
//     occluding it from hit-testing; tap back -> it pops, state intact
// ============================================================================

#[test]
fn tapping_the_details_button_pushes_a_route_that_hides_the_home_route_from_hit_testing() {
    let mut demo = MountedDemo::mount();

    assert!(
        demo.find_text(tree::DETAILS_ROUTE_TEXT).is_none(),
        "the details route must not be built before it is pushed"
    );

    // The "+" button's position is captured before the push: the home route
    // stays mounted (`maintain_state` defaults to true) but is skipped from
    // this frame's layout, so its last committed geometry is exactly what a
    // real finger would still see baked into the (now stale, un-hit-testable)
    // screen — the position a user's tap would land on.
    let plus = demo
        .find_text("+")
        .expect("the '+' button's Text must be in the render tree");
    let plus_tap_at = demo.absolute_position(plus);

    let details_button = demo
        .find_text(tree::DETAILS_BUTTON_LABEL)
        .expect("the 'View details' button must be in the render tree");
    let details_tap_at = demo.absolute_position(details_button);
    demo.tap(details_tap_at.dx.get() + 1.0, details_tap_at.dy.get() + 1.0);
    demo.pump(Duration::ZERO);

    assert!(
        demo.find_text(tree::DETAILS_ROUTE_TEXT).is_some(),
        "the details route's content must render once pushed"
    );
    assert!(
        demo.find_text(tree::BACK_BUTTON_LABEL).is_some(),
        "the details route's back button must render once pushed"
    );

    // A tap at the "+" button's old screen position must not reach it: the
    // details `PageRoute` is opaque, so `RenderTheater`'s skip_count now
    // excludes the home route from hit-testing (`overlay/mod.rs::onstage_plan`).
    demo.tap(plus_tap_at.dx.get() + 1.0, plus_tap_at.dy.get() + 1.0);
    demo.pump(Duration::ZERO);
    assert!(
        demo.find_text("Count: 0").is_some(),
        "the home route's '+' button must not be hit-testable while the details route covers it"
    );
    assert!(
        demo.find_text("Count: 1").is_none(),
        "a tap that lands on the covered home route must not reach its counter"
    );
}

#[test]
fn tapping_back_pops_the_details_route_and_preserves_counter_state() {
    let mut demo = MountedDemo::mount();

    // Two increments before navigating away — the state this round-trip must
    // preserve, not the framework's default (`Count: 0`).
    let plus = demo
        .find_text("+")
        .expect("the '+' button's Text must be in the render tree");
    let plus_tap_at = demo.absolute_position(plus);
    demo.tap(plus_tap_at.dx.get() + 1.0, plus_tap_at.dy.get() + 1.0);
    demo.pump(Duration::ZERO);
    demo.tap(plus_tap_at.dx.get() + 1.0, plus_tap_at.dy.get() + 1.0);
    demo.pump(Duration::ZERO);
    assert!(
        demo.find_text("Count: 2").is_some(),
        "two taps precede the push"
    );

    let details_button = demo
        .find_text(tree::DETAILS_BUTTON_LABEL)
        .expect("the 'View details' button must be in the render tree");
    let details_tap_at = demo.absolute_position(details_button);
    demo.tap(details_tap_at.dx.get() + 1.0, details_tap_at.dy.get() + 1.0);
    demo.pump(Duration::ZERO);
    assert!(demo.find_text(tree::DETAILS_ROUTE_TEXT).is_some());

    let back = demo
        .find_text(tree::BACK_BUTTON_LABEL)
        .expect("the back button must be in the render tree while the details route is on top");
    let back_tap_at = demo.absolute_position(back);
    demo.tap(back_tap_at.dx.get() + 1.0, back_tap_at.dy.get() + 1.0);
    demo.pump(Duration::ZERO);

    assert!(
        demo.find_text(tree::DETAILS_ROUTE_TEXT).is_none(),
        "the details route must be gone once popped"
    );
    // The discriminating assertion: `count`/`expanded`/`scroll_offset` are
    // `Rc<Cell<_>>`s captured once by the seed closure in `DemoRootState`
    // (`tree.rs`), so a display check on them alone reads back correctly
    // whether `DemoHomeState` survived the round trip or was torn down and
    // rebuilt from those same closure-held cells — it cannot tell the two
    // apart. `home_create_count` can: it is incremented once per real
    // `DemoHomeState::create_state` call. A value of `2` here would mean the
    // covering `PageRoute` unmounted the home route while it was covered
    // (Flutter's `maintainState == false` path) and popping rebuilt it from
    // scratch; `1` is the only value consistent with the state object itself
    // having survived the whole push+pop round trip.
    assert_eq!(
        demo.home_create_count(),
        1,
        "DemoHomeState::create_state must have run exactly once — across the whole \
         mount, push, and pop — proving the home route's state survived being \
         covered rather than being torn down and rebuilt"
    );
    assert!(
        demo.find_text("Count: 2").is_some(),
        "and, now that create_state's single run is pinned above, the counter \
         correctly shows the pre-navigation count rather than a reset one"
    );

    // The home route's own hit-testing must be restored, too.
    demo.tap(plus_tap_at.dx.get() + 1.0, plus_tap_at.dy.get() + 1.0);
    demo.pump(Duration::ZERO);
    assert!(
        demo.find_text("Count: 3").is_some(),
        "the '+' button must be hit-testable again once the covering route is popped"
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
