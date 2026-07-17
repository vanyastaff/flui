//! Acceptance test for the Material sample app — the Catalog.1 Material
//! exit criterion: "A Material sample app (`Scaffold` + `AppBar` +
//! `FloatingActionButton` + a `ListView` of `Card`s + a `Dialog`) renders
//! and is interactive."
//!
//! `#[path]`-includes the exact tree `examples/material_demo/main.rs` runs
//! (not a duplicate) and mounts it through `flui_binding::HeadlessBinding`'s
//! public surface, then drives it the way an app author's fingers would: tap
//! the floating action button, fill and dismiss its dialog, tap a card,
//! push/pop the settings route, drag the list — asserting on the resulting
//! render tree, not merely "no panic".
//!
//! This test lives in the root crate (not `flui-material`'s own `tests/`)
//! for the same reason `tests/vertical_slice_demo.rs` does: it re-bootstraps
//! a headless tree from `flui-view`/`flui-rendering`/`flui-binding`'s public
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
mod tree;

use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant};

use flui_binding::HeadlessBinding;
use flui_foundation::RenderId;
use flui_interaction::events::{PointerType, make_down_event, make_move_event, make_up_event};
use flui_material::back_button::back_arrow_icon_data;
use flui_material::{Theme, ThemeData};
use flui_rendering::constraints::BoxConstraints;
use flui_rendering::hit_testing::HitTestResult;
use flui_rendering::pipeline::PipelineOwner;
use flui_rendering::testing::inspect;
use flui_types::geometry::px;
use flui_types::{Offset, Size};
use flui_view::{BuildOwner, ElementTree};
use flui_widgets::{MediaQuery, MediaQueryData, VsyncScope};
use parking_lot::RwLock;

/// The mounted root's logical width — wide enough for a card row, narrow
/// enough that the FAB's end-float offset from the trailing edge is easy to
/// pin down exactly (see [`FAB_MARGIN`]/`FAB_SIZE` in
/// `scaffold_mounts_with_app_bar_at_top_and_fab_at_the_end_float_position`).
const ROOT_WIDTH: f32 = 480.0;
/// The mounted root's logical height — tall enough to show several cards but
/// short enough that [`tree::INITIAL_ITEM_COUNT`] cards (at
/// [`tree::ITEM_EXTENT`] each) genuinely overflow it, so the drag-to-scroll
/// test exercises a real overflow.
const ROOT_HEIGHT: f32 = 800.0;

fn root_constraints() -> BoxConstraints {
    BoxConstraints::tight(Size::new(px(ROOT_WIDTH), px(ROOT_HEIGHT)))
}

/// Everything the test needs to drive and inspect the mounted demo tree.
struct MountedDemo {
    binding: HeadlessBinding,
    pipeline_owner: Arc<RwLock<PipelineOwner>>,
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

        let mut build_owner = BuildOwner::new();
        let mut tree = ElementTree::new();
        let pipeline_owner = Arc::new(RwLock::new(PipelineOwner::new()));

        // Install the async-driver / post-frame / interaction-dispatch
        // capabilities on this binding's owner BEFORE the mount build pass —
        // `MaterialDemoHomeState::init_state` calls `ctx.rebuild_handle()`,
        // which needs the owner already wired to `binding`'s scheduler.
        binding.install_build_capabilities(&mut build_owner);

        let scoped_root = VsyncScope::new(binding.vsync().clone(), wrapped_root);

        binding.enter_owner_scope(|| {
            let root_element = tree.mount_root_with_pipeline_owner(
                &scoped_root,
                Some(Arc::clone(&pipeline_owner)),
                &mut build_owner.element_owner_mut(),
            );
            build_owner.schedule_build_for(root_element, 0);
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

    /// How many times `MaterialDemoHomeState::create_state` has run so far.
    fn home_create_count(&self) -> u32 {
        self.home_create_count.get()
    }

    /// Drive one deterministic frame.
    fn pump(&mut self, dt: Duration) {
        self.binding.pump_frame(dt);
    }

    /// Hit-test at root-local `(x, y)` and dispatch a synthetic pointer-down.
    fn tap_down(&self, x: f32, y: f32) {
        self.dispatch_at(make_down_event(offset(x, y), PointerType::Mouse), x, y);
    }

    /// Hit-test at root-local `(x, y)` and dispatch a synthetic pointer-up —
    /// paired with [`tap_down`](Self::tap_down) at the same position, this
    /// completes a tap.
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

    /// Taps the center of `id`'s rendered box — the standard way this test
    /// drives a button whose own on-screen glyph text was used only to find
    /// it (see [`find_text`](Self::find_text)'s callers).
    fn tap_node(&self, id: RenderId) {
        let position = self.absolute_position(id);
        self.tap(position.dx.get() + 1.0, position.dy.get() + 1.0);
    }

    /// Hit-test at root-local `(x, y)` and dispatch a synthetic pointer-down,
    /// advancing the gesture clock first so the drag recognizer's first
    /// velocity-tracker sample gets a fresh timestamp (see
    /// [`advance_gesture_clock`]).
    fn drag_down(&self, x: f32, y: f32) {
        advance_gesture_clock();
        self.dispatch_at(make_down_event(offset(x, y), PointerType::Mouse), x, y);
    }

    /// Hit-test at root-local `(x, y)` and dispatch a synthetic pointer-move,
    /// advancing the gesture clock first (see [`advance_gesture_clock`]).
    fn drag_move(&self, x: f32, y: f32) {
        advance_gesture_clock();
        self.dispatch_at(make_move_event(offset(x, y), PointerType::Mouse), x, y);
    }

    /// Hit-test at root-local `(x, y)` and dispatch a synthetic pointer-up —
    /// pairs with [`drag_down`](Self::drag_down)/[`drag_move`](Self::drag_move)
    /// to complete a drag gesture.
    fn drag_up(&self, x: f32, y: f32) {
        self.dispatch_at(make_up_event(offset(x, y), PointerType::Mouse), x, y);
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

    /// Every render node whose short type name (generic parameters stripped)
    /// equals `render_type_name` — duplicated from
    /// `crates/flui-material/tests/common/mod.rs`'s `LaidOut::find_all_by_render_type`
    /// for the same reason every other helper here is (see the module doc).
    fn find_all_by_render_type(&self, render_type_name: &str) -> Vec<RenderId> {
        let owner = self.pipeline_owner.read();
        owner
            .render_tree()
            .iter()
            .filter_map(|(id, _node)| {
                let diagnostics = owner.debug_node_diagnostics(id)?;
                let short_name = diagnostics.name()?.split('<').next().unwrap_or("");
                (short_name == render_type_name).then_some(id)
            })
            .collect()
    }

    /// The laid-out size of a render node.
    fn size(&self, id: RenderId) -> Size {
        inspect::box_geometry(&self.pipeline_owner.read(), id)
            .expect("render node should have box geometry after layout")
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
            if let Some(offset) = flui_rendering::testing::inspect::render_offset(&owner, current) {
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

/// Spin until `Instant::now()` returns a value strictly greater than the one
/// returned by the immediately preceding call.
///
/// Mirrors `tests/vertical_slice_demo.rs`'s identical helper (unreachable
/// from here — see the module doc). `DragGestureRecognizer::handle_move`
/// timestamps every velocity-tracker sample with `Instant::now()`; two
/// dispatches landing in the same OS timer tick make the least-squares
/// velocity fit singular (NaN). Calling this before each down/move dispatch
/// that should count toward velocity guarantees consecutive samples get
/// strictly increasing timestamps.
fn advance_gesture_clock() {
    let t0 = Instant::now();
    while Instant::now() == t0 {
        std::hint::spin_loop();
    }
}

/// The app bar action's glyph text, computed from the same [`tree::settings_icon_data`]
/// the mounted tree itself draws — so this test locates the real rendered
/// node rather than guessing its position.
fn settings_glyph_text() -> String {
    tree::settings_icon_data()
        .code_point_string()
        .expect("the settings glyph's codepoint must be a valid Unicode scalar value")
}

/// The app bar action's glyph text for [`tree::tabs_icon_data`] — same
/// reasoning as [`settings_glyph_text`].
fn tabs_glyph_text() -> String {
    tree::tabs_icon_data()
        .code_point_string()
        .expect("the tabs glyph's codepoint must be a valid Unicode scalar value")
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

#[test]
fn scaffold_mounts_with_app_bar_at_top_and_fab_at_the_end_float_position() {
    let demo = MountedDemo::mount();

    let title = demo
        .find_text(tree::APP_TITLE)
        .expect("the app bar's title must render");
    let title_position = demo.absolute_position(title);
    assert!(
        title_position.dy.get() < flui_material::app_bar::DEFAULT_TOOLBAR_HEIGHT,
        "the app bar's title must sit within the toolbar's own height band, got y={}",
        title_position.dy.get(),
    );

    let fab_glyph = demo
        .find_text(tree::FAB_LABEL)
        .expect("the FAB's '+' label must render");
    let fab_position = demo.absolute_position(fab_glyph);

    // `FloatingActionButtonLocation.endFloat`: 16px from the trailing and
    // bottom edges (`scaffold.rs`'s `FLOATING_ACTION_BUTTON_MARGIN`), with no
    // `MediaQuery` padding/view-insets in this mount (`MediaQueryData::default()`)
    // — see `scaffold.rs`'s `ScaffoldLayoutDelegate::perform_layout` for the
    // exact formula this pins.
    const FAB_MARGIN: f32 = 16.0;
    let expected_x = ROOT_WIDTH - FAB_MARGIN - flui_material::floating_action_button::FAB_SIZE;
    let expected_y = ROOT_HEIGHT - FAB_MARGIN - flui_material::floating_action_button::FAB_SIZE;
    assert!(
        (fab_position.dx.get() - expected_x).abs() < 1.0,
        "the FAB must float {FAB_MARGIN}px from the trailing edge, expected x={expected_x}, got \
         x={}",
        fab_position.dx.get(),
    );
    assert!(
        (fab_position.dy.get() - expected_y).abs() < 1.0,
        "the FAB must float {FAB_MARGIN}px from the bottom edge, expected y={expected_y}, got \
         y={}",
        fab_position.dy.get(),
    );
}

// ============================================================================
// (2) Tapping the FAB opens the dialog; the page beneath becomes
//     un-hit-testable while the dialog's barrier covers it
// ============================================================================

#[test]
fn tapping_the_fab_opens_the_dialog_and_hides_the_page_beneath_from_hit_testing() {
    let mut demo = MountedDemo::mount();

    // Capture the settings action's position BEFORE the dialog covers it —
    // the home route stays mounted, laid out, and painted underneath a
    // `PopupRoute` (`opaque: false`), so its last committed geometry is
    // exactly what a real finger would still see baked into the (now stale,
    // un-hit-testable) screen.
    let settings_glyph = settings_glyph_text();
    let settings_button = demo
        .find_text(&settings_glyph)
        .expect("the app bar's settings action must render before the dialog opens");
    let settings_position = demo.absolute_position(settings_button);

    let fab = demo
        .find_text(tree::FAB_LABEL)
        .expect("the FAB must render");
    demo.tap_node(fab);
    demo.pump(Duration::ZERO);

    assert!(
        demo.find_text(tree::ADD_DIALOG_TITLE).is_some(),
        "the AlertDialog's title must render once the FAB opens it"
    );
    assert!(
        demo.find_text(tree::CANCEL_LABEL).is_some() && demo.find_text(tree::ADD_LABEL).is_some(),
        "the dialog's Cancel/Add actions must render"
    );

    // A tap at the settings action's old screen position must not reach it:
    // `show_dialog`'s barrier sits on top of the whole screen and is
    // `barrier_dismissible: true`, so the tap is consumed by the barrier
    // itself (popping the dialog) rather than falling through to the covered
    // home route beneath it — proof the home route is genuinely
    // un-hit-testable, not merely that this one tap happened to miss it.
    demo.tap(
        settings_position.dx.get() + 1.0,
        settings_position.dy.get() + 1.0,
    );
    demo.pump(Duration::ZERO);
    assert!(
        demo.find_text(tree::SETTINGS_ROUTE_TITLE).is_none(),
        "a tap that lands on the covered home route's settings action must not reach it — the \
         dialog's barrier must have absorbed it instead of pushing the settings route"
    );
    assert!(
        demo.find_text(tree::ADD_DIALOG_TITLE).is_none(),
        "the dismissible barrier itself must have popped the dialog on that tap, confirming the \
         barrier (not the home route beneath it) is what caught the pointer"
    );
}

// ============================================================================
// (3) Dialog "Add" appends an item; the home route's state survives the
//     round trip
// ============================================================================

#[test]
fn dialog_add_appends_an_item_and_preserves_home_state() {
    let mut demo = MountedDemo::mount();
    assert_eq!(
        demo.home_create_count(),
        1,
        "MaterialDemoHomeState::create_state must have run exactly once at mount"
    );
    assert!(
        demo.find_text("Item 20").is_none(),
        "the list must start with exactly INITIAL_ITEM_COUNT (20) items"
    );

    let fab = demo
        .find_text(tree::FAB_LABEL)
        .expect("the FAB must render");
    demo.tap_node(fab);
    demo.pump(Duration::ZERO);
    assert!(demo.find_text(tree::ADD_DIALOG_TITLE).is_some());

    let add_button = demo
        .find_text(tree::ADD_LABEL)
        .expect("the dialog's Add action must render");
    demo.tap_node(add_button);
    demo.pump(Duration::ZERO);

    assert!(
        demo.find_text(tree::ADD_DIALOG_TITLE).is_none(),
        "the dialog must be gone once Add pops it"
    );
    assert!(
        demo.find_text("Item 20").is_some(),
        "Add must append a fresh item (the 21st, labeled 'Item 20') to the list"
    );
    // The discriminating assertion: `items` is an `Rc<RefCell<_>>` shared
    // with the seed closure (`tree.rs`'s `MaterialDemoRoot::home_create_count`
    // doc), so a display check on the appended item alone reads back
    // correctly whether `MaterialDemoHomeState` survived the dialog round
    // trip or was torn down and rebuilt from those same closure-held cells —
    // it cannot tell the two apart. `home_create_count` can.
    assert_eq!(
        demo.home_create_count(),
        1,
        "MaterialDemoHomeState::create_state must not re-run across a PopupRoute round trip — \
         PopupRoute's opaque: false keeps the home route mounted the whole time"
    );
}

// ============================================================================
// (3b) Add shows a snack bar via the scope-mounted ScaffoldMessenger, which
//      auto-dismisses after its own display duration
// ============================================================================

#[test]
fn adding_an_item_shows_a_snack_bar_that_auto_dismisses() {
    let mut demo = MountedDemo::mount();

    let fab = demo
        .find_text(tree::FAB_LABEL)
        .expect("the FAB must render");
    demo.tap_node(fab);
    demo.pump(Duration::ZERO);
    assert!(
        demo.find_text(tree::ADD_DIALOG_TITLE).is_some(),
        "the FAB must still open the Add-item dialog with the ScaffoldMessenger mounted above it"
    );

    let add_button = demo
        .find_text(tree::ADD_LABEL)
        .expect("the dialog's Add action must render");
    demo.tap_node(add_button);
    demo.pump(Duration::ZERO);

    assert!(
        demo.find_text(tree::ADD_DIALOG_TITLE).is_none(),
        "the dialog must still close on Add"
    );

    // Pumped in small (one simulated frame each) steps rather than a few
    // large jumps: the entrance -> display-timer -> exit sequence spawns a
    // fresh controller mid-sequence (`ScaffoldMessengerHandle`'s display
    // timer starts only once the entrance controller's own `Completed`
    // status is observed), so a single huge `pump` would not give that
    // freshly-registered controller its fair share of the elapsed time —
    // matching `crates/flui-material/tests/snack_bar.rs`'s own
    // frame-stepped `pump_ms` helper.
    let frame = Duration::from_millis(16);
    let pump_ms = |demo: &mut MountedDemo, millis: u64| {
        let frames = (millis / frame.as_millis() as u64) + 2;
        for _ in 0..frames {
            demo.pump(frame);
        }
    };

    // Carry the 250ms entrance past its end. (The snack bar's content
    // mounts immediately, at `heightFactor: 0` — `find_text` sees it
    // regardless of animated height, so the meaningful assertion is that
    // it survives well past the entrance, not that it's absent before it.)
    pump_ms(&mut demo, 250);
    assert!(
        demo.find_text(tree::SNACK_BAR_ADDED_MESSAGE).is_some(),
        "the snack bar must be shown once its entrance animation completes"
    );

    // Comfortably before its 4s default display duration elapses: still shown.
    pump_ms(&mut demo, 2000);
    assert!(
        demo.find_text(tree::SNACK_BAR_ADDED_MESSAGE).is_some(),
        "the snack bar must still be visible well before its display duration elapses"
    );

    // Past the 4s display duration plus the 250ms exit reverse: gone.
    pump_ms(&mut demo, 2500);
    assert!(
        demo.find_text(tree::SNACK_BAR_ADDED_MESSAGE).is_none(),
        "the snack bar must have auto-dismissed once its display duration elapsed"
    );
}

// ============================================================================
// (4) Dialog "Cancel" dismisses without appending
// ============================================================================

#[test]
fn dialog_cancel_dismisses_without_appending() {
    let mut demo = MountedDemo::mount();

    let fab = demo
        .find_text(tree::FAB_LABEL)
        .expect("the FAB must render");
    demo.tap_node(fab);
    demo.pump(Duration::ZERO);
    assert!(demo.find_text(tree::ADD_DIALOG_TITLE).is_some());

    let cancel_button = demo
        .find_text(tree::CANCEL_LABEL)
        .expect("the dialog's Cancel action must render");
    demo.tap_node(cancel_button);
    demo.pump(Duration::ZERO);

    assert!(
        demo.find_text(tree::ADD_DIALOG_TITLE).is_none(),
        "the dialog must be gone once Cancel pops it"
    );
    assert!(
        demo.find_text("Item 20").is_none(),
        "Cancel must not append any item"
    );
    assert!(
        demo.find_text("Item 0").is_some(),
        "the original items must be untouched"
    );
}

// ============================================================================
// (5) Tapping a Card updates the selected-item display
// ============================================================================

#[test]
fn tapping_a_card_updates_the_selected_item_display() {
    let mut demo = MountedDemo::mount();

    assert!(
        demo.find_text("Selected: none").is_some(),
        "no card is selected before any tap"
    );

    let item = demo
        .find_text("Item 3")
        .expect("the third card's label must render");
    demo.tap_node(item);
    demo.pump(Duration::ZERO);

    assert!(
        demo.find_text("Selected: none").is_none(),
        "the stale 'Selected: none' text must be gone after the tap rebuilds it"
    );
    assert!(
        demo.find_text("Selected: Item 3").is_some(),
        "tapping 'Item 3' must update the selected-item display to name it"
    );

    // A second, different card keeps the display live — proves the home
    // route's state survives across rebuilds rather than resetting.
    let other_item = demo
        .find_text("Item 7")
        .expect("the seventh card's label must render");
    demo.tap_node(other_item);
    demo.pump(Duration::ZERO);
    assert!(demo.find_text("Selected: Item 7").is_some());
    assert!(demo.find_text("Selected: Item 3").is_none());
}

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

/// Real per-move drag threshold for `GestureDetector`'s pan recognizer
/// (`DragAxis::Free`) — see `tests/vertical_slice_demo.rs`'s identical
/// constant/comment for why 18px (the touch default), not the mouse default,
/// is the operative slop even though these events dispatch as
/// `PointerType::Mouse`.
const DRAG_SLOP: f32 = 18.0;

#[test]
fn dragging_inside_the_list_scrolls_its_items() {
    let mut demo = MountedDemo::mount();

    let item0 = demo
        .find_text("Item 0")
        .expect("the first card's label must render");
    let offset_before = demo.absolute_position(item0);

    // A drag anchor safely inside the list body: below the app bar (56px)
    // and the "Selected: …" row, above the FAB.
    let anchor_x = ROOT_WIDTH / 2.0;
    let anchor_y = ROOT_HEIGHT / 2.0;

    const SLOP_CROSSING_DELTA: f32 = DRAG_SLOP + 7.0; // 25.0, safely > 18.0
    const UPDATE_DELTA_1: f32 = 20.0;
    const UPDATE_DELTA_2: f32 = 25.0;
    let expected_scroll_delta = UPDATE_DELTA_1 + UPDATE_DELTA_2;

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
    // `dy`) — matching `Scrollable`'s own pan-update wiring and
    // `tests/vertical_slice_demo.rs`'s identical list-drag test.
    let moved_up_by = offset_before.dy.get() - offset_after.dy.get();

    assert!(
        (moved_up_by - expected_scroll_delta).abs() < 1.0,
        "dragging up {expected_scroll_delta}px worth of post-slop deltas must move item 0's \
         paint position up by the same amount (the slop-crossing move's delta is swallowed): \
         before={offset_before:?}, after={offset_after:?}, moved_up_by={moved_up_by}"
    );
}

// ============================================================================
// (8) Tabs route — TabBarView + AppBar.bottom
// ============================================================================

/// Pushes [`tree::tabs_route`] from the home route's app bar action and
/// returns once the tabs route's title has rendered — the shared setup
/// every test below starts from.
fn push_tabs_route(demo: &mut MountedDemo) {
    let tabs_glyph = tabs_glyph_text();
    let tabs_button = demo
        .find_text(&tabs_glyph)
        .expect("the app bar's tabs action must render");
    demo.tap_node(tabs_button);
    demo.pump(Duration::ZERO);

    assert!(
        demo.find_text(tree::TABS_ROUTE_TITLE).is_some(),
        "the tabs route's app bar title must render once pushed"
    );
}

/// Taps a tab's own label text in the mounted `TabBar` — every test below
/// needs this at least once, so it's factored out rather than copy-pasted
/// three times.
fn tap_tab(demo: &mut MountedDemo, tab_label: &str) {
    let label = demo
        .find_text(tab_label)
        .unwrap_or_else(|| panic!("tab label {tab_label:?} must render in the mounted TabBar"));
    demo.tap_node(label);
    demo.pump(Duration::ZERO);
}

/// A tab tap switches the visible child: on mount, tab 0
/// ([`tree::OVERVIEW_TAB_LABEL`])'s content is showing and neither other
/// tab's is; tapping tab 1 ([`tree::COUNTER_TAB_LABEL`]) swaps which one is.
#[test]
fn tapping_a_tab_switches_the_visible_child() {
    let mut demo = MountedDemo::mount();
    push_tabs_route(&mut demo);

    assert!(
        demo.find_text(tree::OVERVIEW_TAB_TEXT).is_some(),
        "tab 0 (Overview) is the controller's initial index — its content must be built and \
         showing on mount"
    );
    assert!(
        demo.find_text(&format!("{}0", tree::COUNTER_LABEL_PREFIX))
            .is_none(),
        "the Counter tab must not be showing before it's ever tapped"
    );

    tap_tab(&mut demo, tree::COUNTER_TAB_LABEL);

    assert!(
        demo.find_text(&format!("{}0", tree::COUNTER_LABEL_PREFIX))
            .is_some(),
        "tapping the Counter tab must switch the visible child to its content"
    );
}

/// The Counter tab's count survives switching away to another tab and back —
/// `TabBarView`'s `Offstage` keep-alive retention, not a rebuild that resets
/// local state. Flutter parity target: `TabBarView`'s own "state survives a
/// tab switch" contract (see `tab_bar_view.rs`'s module docs for the
/// composed mechanism this substrate uses instead of a real `PageView`).
#[test]
fn the_counters_state_survives_switching_away_and_back() {
    let mut demo = MountedDemo::mount();
    push_tabs_route(&mut demo);

    tap_tab(&mut demo, tree::COUNTER_TAB_LABEL);
    let increment = demo
        .find_text(tree::COUNTER_INCREMENT_LABEL)
        .expect("the Counter tab's Increment button must render once visited");
    // The tap position is captured ONCE, up front, and reused for every
    // subsequent tap — re-resolving `absolute_position` off a `RenderId`
    // AFTER a rebuild it triggered is unreliable (the node's parent chain
    // can read back detached until the next full relayout), the same
    // fixed-position-computed-once pattern
    // `tests/vertical_slice_demo.rs`'s `tapping_the_plus_button_updates_the_rendered_counter_text`
    // already established for its own repeatedly-tapped counter button.
    let tap_at = demo.absolute_position(increment);
    demo.tap(tap_at.dx.get() + 1.0, tap_at.dy.get() + 1.0);
    demo.pump(Duration::ZERO);
    demo.tap(tap_at.dx.get() + 1.0, tap_at.dy.get() + 1.0);
    demo.pump(Duration::ZERO);
    demo.tap(tap_at.dx.get() + 1.0, tap_at.dy.get() + 1.0);
    demo.pump(Duration::ZERO);

    assert!(
        demo.find_text(&format!("{}3", tree::COUNTER_LABEL_PREFIX))
            .is_some(),
        "three taps on Increment must bring the count to 3"
    );

    // Switch away to Overview, then back to Counter. `TabBarView`'s
    // `Offstage` keep-alive retention (see `tab_bar_view.rs`'s module docs)
    // means the Counter tab's `RenderParagraph` stays in the tree the whole
    // time — merely unpainted while inactive, not torn down — so a
    // "must not be found while inactive" assertion here would test the
    // wrong thing (`find_text` has no visibility/offstage awareness; that
    // half of the contract is `crates/flui-material/tests/tab_bar_view.rs`'s
    // `default_tab_controller_ancestor_drives_the_active_child_through_offstage`
    // via the `RenderOffstage` diagnostics flag directly). The genuinely
    // observable retention proof at this level is that the count reads 3
    // again after the round trip, not 0 — a reset here is exactly what a
    // regression to index-keyed (rebuild-from-scratch) tab elements would
    // produce.
    tap_tab(&mut demo, tree::OVERVIEW_TAB_LABEL);
    tap_tab(&mut demo, tree::COUNTER_TAB_LABEL);

    assert!(
        demo.find_text(&format!("{}3", tree::COUNTER_LABEL_PREFIX))
            .is_some(),
        "the count must still read 3 after switching away and back — retention, not a reset"
    );
}

/// The About tab (index 2) is never built until it's actually visited —
/// `TabBarView`'s lazy-build contract, proven end to end through the real
/// `TabBar`'s tap dispatch (not just `TabBarView` mounted directly, as
/// `crates/flui-material/tests/tab_bar_view.rs`'s own
/// `a_tab_is_not_built_until_it_becomes_active` already covers in
/// isolation).
#[test]
fn the_about_tab_is_not_built_until_visited() {
    let mut demo = MountedDemo::mount();
    push_tabs_route(&mut demo);

    assert!(
        demo.find_text(tree::ABOUT_TAB_TEXT).is_none(),
        "the About tab's content must not be built before it's ever visited"
    );

    tap_tab(&mut demo, tree::ABOUT_TAB_LABEL);

    assert!(
        demo.find_text(tree::ABOUT_TAB_TEXT).is_some(),
        "the About tab's content must be built once it's visited"
    );
}

/// With the `TabBar` mounted as `AppBar.bottom`, the app bar's total height
/// is `toolbar_height (56) + the TabBar's own preferred height (48, three
/// plain-text tabs: `TAB_HEIGHT` 46 + `indicator_weight` 2)` — the same
/// `toolbar_height + bottom_height` math `crates/flui-material/tests/app_bar.rs`
/// pins in isolation, now proven reachable through the full sample-app tree
/// (real `Theme`/`MediaQuery` ancestors, a real `Navigator`-pushed route).
#[test]
fn the_app_bar_height_is_toolbar_plus_the_mounted_tab_bars_height() {
    let mut demo = MountedDemo::mount();
    push_tabs_route(&mut demo);

    const EXPECTED_HEIGHT: f32 = 56.0 + 48.0;
    let matches = demo
        .find_all_by_render_type("RenderConstrainedBox")
        .into_iter()
        .filter(|&id| demo.size(id).height == px(EXPECTED_HEIGHT))
        .count();
    assert!(
        matches > 0,
        "expected at least one RenderConstrainedBox sized to toolbar_height + TabBar height \
         ({EXPECTED_HEIGHT}px) once the tabs route is mounted"
    );
}

// ============================================================================
// `tree.rs` sanity — both `#[path]` consumers reference the same symbols
// ============================================================================

/// `MaterialDemoApp` (the thin `StatelessView` `flui_app::run_app` entry
/// point) is exercised at runtime only by `examples/material_demo/main.rs`,
/// not this headless test — the acceptance tests above mount
/// `MaterialDemoRoot` directly (see `MountedDemo::mount`'s doc). Referencing
/// it here keeps both `#[path]` consumers of `tree.rs` compiling the same
/// symbol set, so a signature change that breaks the example's entry point
/// fails `cargo test` too, not only `cargo build --example`.
#[test]
fn demo_app_entry_point_constructs() {
    let _ = tree::MaterialDemoApp;
}
