//! Acceptance test for the vertical-slice demo.
//!
//! `#[path]`-includes the exact tree `examples/vertical_slice_demo/main.rs`
//! runs (not a duplicate) and mounts it through `flui_testing::HeadlessBinding`'s
//! public surface, then drives it the way an app author's fingers would: tap
//! the "+" button, change the list's scroll position, tap the animated box —
//! asserting on the resulting render tree, not merely "no panic".
//!
//! `flui-widgets`' `tests/common` harness lives in a different crate (a
//! private integration-test module, unreachable from here): this test lives
//! in the root crate, so it re-bootstraps a headless tree from `flui-view` /
//! `flui-rendering` / `flui-testing`'s public API only, mirroring the
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

use std::time::{Duration, Instant};

use flui_foundation::RenderId;
use flui_foundation::geometry::{Offset, Size};
use flui_interaction::events::{PointerType, make_down_event, make_move_event, make_up_event};
use flui_rendering::constraints::BoxConstraints;
use flui_rendering::hit_testing::HitTestResult;
use flui_rendering::pipeline::{PipelineCell, PipelineOwner};
use flui_rendering::testing::inspect;
use flui_testing::HeadlessBinding;
use flui_testing::bootstrap::{MountOptions, MountOwners};
use flui_widgets::{FocusRoot, GestureArenaScope, VsyncScope};

/// Root constraints the demo is mounted under: wide/tall enough that the
/// counter row, the [`tree::LIST_BOX_HEIGHT`]-tall list box, and the
/// animated box (up to [`tree::EXPANDED_HEIGHT`] tall) all coexist in the
/// `Column` without overflow.
fn root_constraints() -> BoxConstraints {
    BoxConstraints::tight(Size::new(480.0, 720.0))
}

/// Everything the test needs to drive and inspect the mounted demo tree.
struct MountedDemo {
    binding: HeadlessBinding,
    pipeline_owner: PipelineCell,
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

        let pipeline_owner = PipelineCell::new(PipelineOwner::new());

        let focused_root = FocusRoot::new(root_view);
        let animated_root = VsyncScope::new(binding.vsync().clone(), focused_root);
        let scoped_root = GestureArenaScope::new(binding.arena().clone(), animated_root);

        // Through `flui-testing`'s canonical bootstrap: it owns the ordering
        // (capabilities before the mount — this tree's `init_state` calls
        // `ctx.rebuild_handle()` — render-root discovery, the layout<->build
        // fixpoint, the lazy-sliver service pass, then the bind). Copies of that
        // sequence have drifted silently before.
        binding.mount_root(
            &scoped_root,
            MountOwners::with_pipeline_owner(pipeline_owner.clone()),
            MountOptions::new(root_constraints()),
        );

        Self {
            binding,
            pipeline_owner,
        }
    }

    /// Drive one deterministic frame.
    fn pump(&mut self, dt: Duration) {
        self.binding.pump_frame(dt);
    }

    /// Hit-test at root-local `(x, y)` and dispatch a synthetic pointer-down.
    fn tap_down(&self, x: f64, y: f64) {
        self.dispatch_pointer(make_down_event(offset(x, y), PointerType::Mouse));
    }

    /// Hit-test at root-local `(x, y)` and dispatch a synthetic pointer-up —
    /// paired with [`tap_down`](Self::tap_down) at the same position, this
    /// completes a tap (`TapGestureRecognizer` fires `on_tap`).
    fn tap_up(&self, x: f64, y: f64) {
        self.dispatch_pointer(make_up_event(offset(x, y), PointerType::Mouse));
    }

    fn hit_test(&self, position: Offset) -> HitTestResult {
        self.pipeline_owner.with(|owner| {
            let mut result = HitTestResult::new();
            owner.hit_test(position, &mut result);
            result
        })
    }

    fn dispatch_pointer(&self, event: flui_interaction::PointerEvent) {
        self.binding
            .dispatch_pointer(&event, |position| self.hit_test(position));
    }

    /// A full tap (down + up) at `(x, y)`.
    fn tap(&self, x: f64, y: f64) {
        self.tap_down(x, y);
        self.tap_up(x, y);
    }

    /// Hit-test at root-local `(x, y)` and dispatch a synthetic pointer-down,
    /// advancing the gesture clock first so the drag recognizer's first
    /// velocity-tracker sample gets a fresh timestamp (see
    /// [`advance_gesture_clock`]). Distinct from [`tap_down`](Self::tap_down):
    /// only drag sequences need the clock advance, and `tap_down` is shared by
    /// unrelated tests this change must not perturb.
    fn drag_down(&self, x: f64, y: f64) {
        advance_gesture_clock();
        self.dispatch_pointer(make_down_event(offset(x, y), PointerType::Mouse));
    }

    /// Hit-test at root-local `(x, y)` and dispatch a synthetic pointer-move,
    /// advancing the gesture clock first (see [`advance_gesture_clock`]).
    fn drag_move(&self, x: f64, y: f64) {
        advance_gesture_clock();
        self.dispatch_pointer(make_move_event(offset(x, y), PointerType::Mouse));
    }

    /// Hit-test at root-local `(x, y)` and dispatch a synthetic pointer-up —
    /// pairs with [`drag_down`](Self::drag_down)/[`drag_move`](Self::drag_move)
    /// to complete a drag gesture.
    fn drag_up(&self, x: f64, y: f64) {
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
        self.pipeline_owner.with(|owner| {
            let queried = base_type_name(render_type_name);
            owner
                .render_tree()
                .iter()
                .filter_map(|(id, _node)| {
                    let diagnostics = owner.debug_node_diagnostics(id)?;
                    (diagnostics.name().map(base_type_name) == Some(queried)).then_some(id)
                })
                .collect()
        })
    }

    /// The unique `RenderParagraph` node whose plain-text content is `text`.
    fn find_text(&self, text: &str) -> Option<RenderId> {
        self.pipeline_owner.with(|owner| {
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
        })
    }

    /// The animated box's own `RenderContainer`.
    ///
    /// The demo tree mounts several `RenderContainer` nodes (the screen
    /// background, the "+" and details buttons, every list item, and the
    /// animated box), so type-name lookup alone is ambiguous. The animated
    /// box is the only one whose *committed width and height are
    /// simultaneously* within `[COLLAPSED, EXPANDED]` on both axes — every
    /// other candidate fails on at least one axis by construction
    /// (`tree.rs`'s constants keep the ranges disjoint).
    ///
    /// # Panics
    /// Panics when zero or more than one candidate matches.
    fn animated_box_render_id(&self) -> RenderId {
        self.pipeline_owner.with(|owner| {
            let width_range = tree::COLLAPSED_WIDTH.min(tree::EXPANDED_WIDTH) - 1.0
                ..=tree::COLLAPSED_WIDTH.max(tree::EXPANDED_WIDTH) + 1.0;
            let height_range = tree::COLLAPSED_HEIGHT.min(tree::EXPANDED_HEIGHT) - 1.0
                ..=tree::COLLAPSED_HEIGHT.max(tree::EXPANDED_HEIGHT) + 1.0;
            let matches: Vec<RenderId> = self
                .find_all_by_render_type("RenderContainer")
                .into_iter()
                .filter(|&id| {
                    inspect::box_geometry(owner, id).is_some_and(|size| {
                        width_range.contains(&size.width) && height_range.contains(&size.height)
                    })
                })
                .collect();
            match matches.as_slice() {
                [id] => *id,
                [] => panic!("no RenderContainer falls within the animated box's size range"),
                _ => panic!(
                    "{} RenderContainer nodes fall within the animated box's size range; \
                 expected exactly one",
                    matches.len()
                ),
            }
        })
    }

    /// The list's fixed-height `SizedBox` wrapper.
    ///
    /// Same disambiguation problem as [`animated_box_render_id`]'s doc
    /// explains, on the other type: the demo's `SizedBox`es each mount a
    /// `RenderConstrainedBox`. This one is the unique node whose committed
    /// height equals [`tree::LIST_BOX_HEIGHT`] (200px) — distinct by
    /// construction from the counter and details spacers (16px).
    ///
    /// # Panics
    /// Panics when zero or more than one candidate matches.
    fn list_box_render_id(&self) -> RenderId {
        self.pipeline_owner.with(|owner| {
            let matches: Vec<RenderId> = self
                .find_all_by_render_type("RenderConstrainedBox")
                .into_iter()
                .filter(|&id| {
                    inspect::box_geometry(owner, id)
                        .is_some_and(|size| (size.height - tree::LIST_BOX_HEIGHT).abs() < 1.0)
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
        })
    }

    /// The list box's root-local center — a safe drag anchor: enough headroom
    /// above and below to cross the slop and keep moving without the pointer
    /// leaving the box (which would hit-test a different render path on the
    /// next move).
    fn list_box_center(&self) -> (f64, f64) {
        let list_box = self.list_box_render_id();
        let size = self
            .pipeline_owner
            .with(|owner| inspect::box_geometry(owner, list_box))
            .expect("the list box must have box geometry after the bootstrap frame");
        let top_left = self.absolute_position(list_box);
        (
            (top_left.dx + size.width / 2.0),
            (top_left.dy + size.height / 2.0),
        )
    }

    /// The screen-space (root-local) top-left of `id`, by summing paint
    /// offsets up the render-tree ancestry — every node between the root and
    /// `id` in this tree only translates (no scale/rotation), so a plain sum
    /// recovers the absolute position.
    fn absolute_position(&self, id: RenderId) -> Offset {
        self.pipeline_owner.with(|owner| {
            let render_tree = owner.render_tree();
            let mut x = 0.0_f64;
            let mut y = 0.0_f64;
            let mut current = id;
            loop {
                if let Some(offset) = inspect::render_offset(owner, current) {
                    x += offset.dx;
                    y += offset.dy;
                }
                match render_tree.parent(current) {
                    Some(parent) => current = parent,
                    None => break,
                }
            }
            offset(x, y)
        })
    }
}

fn offset(x: f64, y: f64) -> Offset {
    Offset::new(x, y)
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
    demo.tap(tap_at.dx + 1.0, tap_at.dy + 1.0);

    // The tap's on_tap handler scheduled a rebuild via `StateCell::update`;
    // the next pump drains it.
    demo.pump(Duration::ZERO);

    assert!(
        demo.find_text("Count: 0").is_none(),
        "the stale 'Count: 0' text must be gone after the tap rebuilds the counter"
    );
    assert!(
        demo.find_text("Count: 1").is_some(),
        "tapping '+' once must rebuild the counter text to 'Count: 1'"
    );

    // A second tap keeps incrementing — proves the element (and its bound
    // `StateCell`) survives across rebuilds rather than being torn down.
    demo.tap(tap_at.dx + 1.0, tap_at.dy + 1.0);
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
const DRAG_SLOP: f64 = 18.0;

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
    const SLOP_CROSSING_DELTA: f64 = DRAG_SLOP + 7.0; // 25.0, safely > 18.0
    const UPDATE_DELTA_1: f64 = 20.0;
    const UPDATE_DELTA_2: f64 = 25.0;
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
    let moved_up_by = offset_before.dy - offset_after.dy;

    assert!(
        (moved_up_by - expected_scroll_delta).abs() < 1.0,
        "dragging a lone recognizer up by {expected_scroll_delta}px must move item 0's paint \
         position up by the same amount: \
         before={offset_before:?}, after={offset_after:?}, moved_up_by={moved_up_by}"
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
    let width_at_rest = demo
        .pipeline_owner
        .with(|owner| inspect::box_geometry(owner, box_id))
        .expect("the animated box must have box geometry after the bootstrap frame")
        .width;
    assert!(
        (width_at_rest - tree::COLLAPSED_WIDTH).abs() < 0.5,
        "the animated box starts at its collapsed width, got {width_at_rest}"
    );

    // Tap the box (Opaque hit-test behavior, so any point inside its bounds
    // works) to toggle `expanded` and retarget the controller.
    let tap_at = demo.absolute_position(box_id);
    demo.tap(tap_at.dx + 2.0, tap_at.dy + 2.0);
    demo.pump(Duration::ZERO); // the detection frame: rebuild + retarget, t = 0

    let box_id = demo.animated_box_render_id();
    let width_after_retarget = demo
        .pipeline_owner
        .with(|owner| inspect::box_geometry(owner, box_id))
        .expect("geometry after retarget")
        .width;
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
        let width = demo
            .pipeline_owner
            .with(|owner| inspect::box_geometry(owner, id))
            .expect("geometry mid-flight")
            .width;
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

// ============================================================================
// `tree.rs` sanity — both `#[path]` consumers reference the same symbols
// ============================================================================

/// `DemoApp` (the thin `StatelessView` `flui_app::run_app` entry point) is
/// exercised at runtime only by `examples/vertical_slice_demo/main.rs`.
/// Referencing it here keeps both `#[path]` consumers of `tree.rs` compiling the
/// same symbol set, so a signature change that breaks the example's entry point
/// fails `cargo test` too, not only `cargo build --example`.
#[test]
fn demo_app_entry_point_constructs() {
    let _ = tree::DemoApp;
}
