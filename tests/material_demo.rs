//! Acceptance test for the Material sample app — the Catalog.1 Material
//! exit criterion: "A Material sample app (`Scaffold` + `AppBar` +
//! `FloatingActionButton` + a `ListView` of `Card`s + a `Dialog`) renders
//! and is interactive."
//!
//! `#[path]`-includes the exact tree `examples/material_demo/main.rs` runs
//! (not a duplicate) and mounts it through `flui_testing::HeadlessBinding`'s
//! public surface, then drives it the way an app author's fingers would: tap
//! the floating action button, fill and dismiss its dialog, tap a card,
//! push/pop the settings route, drag the list — asserting on the resulting
//! render tree, not merely "no panic".
//!
//! This test lives in the root crate (not `flui-material`'s own `tests/`)
//! for the same reason `tests/vertical_slice_demo.rs` does: it re-bootstraps
//! a headless tree from `flui-view`/`flui-rendering`/`flui-testing`'s public
//! API only, mirroring `HeadlessBinding`'s own documented mount sequence.
//! Every helper below (`tap`/`drag_*`/`find_text`/`absolute_position`/
//! `advance_gesture_clock`) is therefore duplicated from
//! `tests/vertical_slice_demo.rs` rather than shared — neither test crate
//! can see the other's private helpers.
//!
//! Honesty notes (Definition of Done) — what this app does **not** exercise,
//! restated from `tree.rs`'s module doc: ink ripple/splash visuals (`InkWell`
//! paints only a static resolved overlay fill), component themes (every
//! widget here rides the fixed M3 baseline), and `SnackBar`/`Drawer` (no
//! `Scaffold` slot exists for either yet).

#[path = "../examples/material_demo/tree.rs"]
#[expect(
    dead_code,
    reason = "the `App` entry-point wrapper is exercised by `demo_layer_snapshots`; this target mounts the demo root directly"
)]
mod tree;

use std::cell::Cell;
use std::rc::Rc;
use std::time::Duration;

use flui_foundation::RenderId;
use flui_foundation::geometry::{Offset, Size};
use flui_interaction::events::{PointerType, make_down_event, make_up_event};
use flui_material::back_button::back_arrow_icon_data;
use flui_material::{Theme, ThemeData};
use flui_rendering::constraints::BoxConstraints;
use flui_rendering::hit_testing::HitTestResult;
use flui_rendering::pipeline::{PipelineCell, PipelineOwner};
use flui_rendering::testing::inspect;
use flui_testing::HeadlessBinding;
use flui_testing::bootstrap::{MountOptions, MountOwners};
use flui_widgets::{FocusRoot, GestureArenaScope, MediaQuery, MediaQueryData, VsyncScope};

/// The mounted root's logical width — wide enough for a card row, narrow
/// enough that the FAB's end-float offset from the trailing edge is easy to
/// pin down exactly (see [`FAB_MARGIN`]/`FAB_SIZE` in
/// `scaffold_mounts_with_app_bar_at_top_and_fab_at_the_end_float_position`).
const ROOT_WIDTH: f64 = 480.0;
/// The mounted root's logical height — tall enough to show several cards but
/// short enough that [`tree::INITIAL_ITEM_COUNT`] cards (at
/// [`tree::ITEM_EXTENT`] each) genuinely overflow it, so the drag-to-scroll
/// test exercises a real overflow.
const ROOT_HEIGHT: f64 = 800.0;

fn root_constraints() -> BoxConstraints {
    BoxConstraints::tight(Size::new(ROOT_WIDTH, ROOT_HEIGHT))
}

/// Everything the test needs to drive and inspect the mounted demo tree.
struct MountedDemo {
    binding: HeadlessBinding,
    pipeline_owner: PipelineCell,
    /// Clone of the mounted [`tree::MaterialDemoRoot`]'s `home_create_count`
    /// — how many times `MaterialDemoHomeState::create_state` has run. See
    /// that field's doc for why this, and not a display assertion, is what
    /// proves state survival across a route push/pop or a dialog round trip.
    home_create_count: Rc<Cell<u32>>,
}

impl MountedDemo {
    /// Mount `tree::demo_root()` wrapped exactly as
    /// [`tree::MaterialDemoApp::build`] wraps it (`MediaQuery(default) ->
    /// Theme(ThemeData::light())`), under a `VsyncScope` over `binding`'s own
    /// registry, run the bootstrap frame, then hand the owners to `binding`.
    ///
    /// Mounting `MaterialDemoRoot` directly (rather than through
    /// `MaterialDemoApp`) is required to capture `home_create_count` before
    /// the tree is wrapped and moved — `MaterialDemoApp` itself carries no
    /// fields to read it back from. The resulting tree is structurally
    /// identical to what `MaterialDemoApp` builds.
    fn mount() -> Self {
        let mut binding = HeadlessBinding::new();

        let root_view = tree::demo_root();
        let home_create_count = Rc::clone(&root_view.home_create_count);
        let wrapped_root = MediaQuery::new(
            MediaQueryData::default(),
            Theme::new(ThemeData::light(), root_view),
        );

        let pipeline_owner = PipelineCell::new(PipelineOwner::new(
            flui_rendering::TextContextHandle::standalone(),
        ));

        let focused_root = FocusRoot::new(wrapped_root);
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
            home_create_count,
        }
    }

    /// How many times `MaterialDemoHomeState::create_state` has run so far.
    fn home_create_count(&self) -> u32 {
        self.home_create_count.get()
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
    /// completes a tap.
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

    /// Taps the center of `id`'s rendered box — the standard way this test
    /// drives a button whose own on-screen glyph text was used only to find
    /// it (see [`find_text`](Self::find_text)'s callers).
    ///
    /// The center, not a corner: a Material button clips to its shape, so a
    /// point just inside the bounding box of a *circular* one can sit outside
    /// the circle and never reach the button at all.
    fn tap_node(&self, id: RenderId) {
        let position = self.absolute_position(id);
        let size = self.size(id);
        self.tap(
            position.dx + size.width / 2.0,
            position.dy + size.height / 2.0,
        );
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

    /// The laid-out size of a render node.
    fn size(&self, id: RenderId) -> Size {
        self.pipeline_owner
            .with(|owner| inspect::box_geometry(owner, id))
            .expect("render node should have box geometry after layout")
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
                if let Some(offset) =
                    flui_rendering::testing::inspect::render_offset(owner, current)
                {
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

/// The app bar action's glyph text, computed from the same [`tree::settings_icon_data`]
/// the mounted tree itself draws — so this test locates the real rendered
/// node rather than guessing its position.
fn settings_glyph_text() -> String {
    tree::settings_icon_data()
        .code_point_string()
        .expect("the settings glyph's codepoint must be a valid Unicode scalar value")
}

/// The implied `BackButton`'s glyph text, computed from
/// `flui_material::back_button::back_arrow_icon_data` — the exact codepoint
/// `AppBar`'s implied leading resolution draws.
fn back_button_glyph_text() -> String {
    back_arrow_icon_data()
        .code_point_string()
        .expect("the back arrow's codepoint must be a valid Unicode scalar value")
}

// ============================================================================
// (1) Scaffold slots present: AppBar at the top, FAB at the endFloat position
// ============================================================================

// ============================================================================
// (2) Tapping the FAB opens the dialog; the page beneath becomes
//     un-hit-testable while the dialog's barrier covers it
// ============================================================================

// ============================================================================
// (3) Dialog "Add" appends an item; the home route's state survives the
//     round trip
// ============================================================================

// ============================================================================
// (3b) Add shows a snack bar via the scope-mounted ScaffoldMessenger, which
//      auto-dismisses after its own display duration
// ============================================================================

// ============================================================================
// (4) Dialog "Cancel" dismisses without appending
// ============================================================================

// ============================================================================
// (5) Tapping a Card updates the selected-item display
// ============================================================================

// ============================================================================
// (6) The app bar action pushes route 2; the implied BackButton pops back
//     with home state intact
// ============================================================================

#[test]
fn app_bar_action_pushes_settings_and_back_button_pops_with_home_state_intact() {
    let mut demo = MountedDemo::mount();

    // Select a card first — the state the round trip must preserve.
    let item = demo
        .find_text("Item 2")
        .expect("the third card's label must render");
    demo.tap_node(item);
    demo.pump(Duration::ZERO);
    assert!(demo.find_text("Selected: Item 2").is_some());

    assert!(
        demo.find_text(tree::SETTINGS_ROUTE_TITLE).is_none(),
        "the settings route must not be built before it is pushed"
    );

    let settings_glyph = settings_glyph_text();
    let settings_button = demo
        .find_text(&settings_glyph)
        .expect("the app bar's settings action must render");
    demo.tap_node(settings_button);
    demo.pump(Duration::ZERO);

    assert!(
        demo.find_text(tree::SETTINGS_ROUTE_TITLE).is_some(),
        "the settings route's app bar title must render once pushed"
    );
    assert!(
        demo.find_text(tree::SETTINGS_ROUTE_TEXT).is_some(),
        "the settings route's body text must render once pushed"
    );

    let back_glyph = back_button_glyph_text();
    let back_button = demo.find_text(&back_glyph).expect(
        "AppBar must synthesize an implied BackButton on the settings route (a poppable \
             navigator ancestor exists there)",
    );
    demo.tap_node(back_button);
    demo.pump(Duration::ZERO);

    assert!(
        demo.find_text(tree::SETTINGS_ROUTE_TITLE).is_none(),
        "the settings route must be gone once the BackButton pops it"
    );
    assert_eq!(
        demo.home_create_count(),
        1,
        "MaterialDemoHomeState::create_state must have run exactly once — across the whole \
         mount, push, and pop — proving the home route's state survived being covered by the \
         opaque settings PageRoute rather than being torn down and rebuilt"
    );
    assert!(
        demo.find_text("Selected: Item 2").is_some(),
        "and, now that create_state's single run is pinned above, the selection correctly shows \
         the pre-navigation choice rather than a reset one"
    );
}

// ============================================================================
// (7) Dragging inside the list scrolls it
// ============================================================================

// ============================================================================
// (8) Tabs route — TabBarView + AppBar.bottom
// ============================================================================

// ============================================================================
// (9) Form route — validated `TextField`, and async load/error/retry/cancel
// ============================================================================

// ============================================================================
// `tree.rs` sanity — both `#[path]` consumers reference the same symbols
// ============================================================================
