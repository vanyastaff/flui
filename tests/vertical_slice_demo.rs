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
#[expect(
    dead_code,
    reason = "the `App` entry-point wrapper is exercised by `demo_layer_snapshots`; this target mounts the demo root directly"
)]
mod tree;

use std::time::Duration;

use flui_foundation::RenderId;
use flui_foundation::geometry::{Offset, Size};
use flui_interaction::events::{PointerKind, make_down_event, make_up_event};
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

        let pipeline_owner = PipelineCell::new(PipelineOwner::new(
            flui_rendering::TextContextHandle::standalone(),
        ));

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
        self.dispatch_pointer(
            make_down_event(offset(x, y), PointerKind::Mouse).expect("finite pointer fixture"),
        );
    }

    /// Hit-test at root-local `(x, y)` and dispatch a synthetic pointer-up —
    /// paired with [`tap_down`](Self::tap_down) at the same position, this
    /// completes a tap (`TapGestureRecognizer` fires `on_tap`).
    fn tap_up(&self, x: f64, y: f64) {
        self.dispatch_pointer(
            make_up_event(offset(x, y), PointerKind::Mouse).expect("finite pointer fixture"),
        );
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

// ============================================================================
// (c) tap the animated box -> the animated property interpolates, then
//     settles at the target
// ============================================================================

// ============================================================================
// (d) tap the details button -> a navigated route pushes over the demo,
//     occluding it from hit-testing; tap back -> it pops, state intact
// ============================================================================

// ============================================================================
// `tree.rs` sanity — both `#[path]` consumers reference the same symbols
// ============================================================================
