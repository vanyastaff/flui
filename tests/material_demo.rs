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
use flui_widgets::{
    FocusRoot, GestureArenaScope, MediaQuery, MediaQueryData, TextEditingController, VsyncScope,
};

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
    /// Clone of the mounted root's Form-route `Name` controller — captured
    /// before mounting so the test can type into it without first pushing
    /// the Form route (see `tree::MaterialDemoRoot::name_controller`'s doc).
    name_controller: TextEditingController,
    /// Clone of the mounted root's Form-route simulated-fetch control.
    fetch_control: tree::SimulatedFetchControl,
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
        let name_controller = root_view.name_controller.clone();
        let fetch_control = root_view.fetch_control.clone();
        let wrapped_root = MediaQuery::new(
            MediaQueryData::default(),
            Theme::new(ThemeData::light(), root_view),
        );

        let pipeline_owner = PipelineCell::new(PipelineOwner::new());

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
            name_controller,
            fetch_control,
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

    /// The unique `RenderParagraph` node whose plain-text content STARTS
    /// WITH `prefix` — for [`find_text`](Self::find_text)'s exact-match
    /// callers, this is the one to reach for when the suffix varies (the
    /// Form route's submitted name).
    fn find_text_with_prefix(&self, prefix: &str) -> Option<RenderId> {
        self.pipeline_owner.with(|owner| {
            let mut found = None;
            for (id, _node) in owner.render_tree().iter() {
                let Some(diagnostics) = owner.debug_node_diagnostics(id) else {
                    continue;
                };
                if diagnostics.name() != Some("RenderParagraph") {
                    continue;
                }
                if diagnostics
                    .get_property("text")
                    .is_some_and(|text| text.starts_with(prefix))
                {
                    assert!(
                        found.is_none(),
                        "multiple RenderParagraph nodes start with {prefix:?}"
                    );
                    found = Some(id);
                }
            }
            found
        })
    }

    /// Every render node whose short type name (generic parameters stripped)
    /// equals `render_type_name` — duplicated from
    /// `packages/flui-material/tests/common/mod.rs`'s `LaidOut::find_all_by_render_type`
    /// for the same reason every other helper here is (see the module doc).
    /// The item count the home list's render object declares — the list is
    /// lazy, so a row below the window is never built and cannot be found by
    /// its text; the count is what an append changes.
    fn list_item_count(&self) -> usize {
        let lists = self.find_all_by_render_type("RenderSliverFixedExtentList");
        assert_eq!(lists.len(), 1, "the home route mounts exactly one list");
        self.pipeline_owner.with(|owner| {
            owner
                .debug_node_diagnostics(lists[0])
                .and_then(|diagnostics| {
                    diagnostics
                        .get_property("item_count")
                        .and_then(|count| count.parse::<usize>().ok())
                })
                .expect("the list reports its item_count")
        })
    }

    fn find_all_by_render_type(&self, render_type_name: &str) -> Vec<RenderId> {
        self.pipeline_owner.with(|owner| {
            owner
                .render_tree()
                .iter()
                .filter_map(|(id, _node)| {
                    let diagnostics = owner.debug_node_diagnostics(id)?;
                    let short_name = diagnostics.name()?.split('<').next().unwrap_or("");
                    (short_name == render_type_name).then_some(id)
                })
                .collect()
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

/// The app bar action's glyph text for [`tree::form_icon_data`] — same
/// reasoning as [`settings_glyph_text`].
fn form_glyph_text() -> String {
    tree::form_icon_data()
        .code_point_string()
        .expect("the form glyph's codepoint must be a valid Unicode scalar value")
}

/// Pushes [`tree::form_route`] from the home route's app bar action and
/// returns once the Form route's title has rendered — the shared setup
/// every Form-route test below starts from. Mirrors [`push_tabs_route`].
fn push_form_route(demo: &mut MountedDemo) {
    let form_glyph = form_glyph_text();
    let form_button = demo
        .find_text(&form_glyph)
        .expect("the app bar's form action must render");
    demo.tap_node(form_button);
    demo.pump(Duration::ZERO);

    assert!(
        demo.find_text(tree::FORM_ROUTE_TITLE).is_some(),
        "the form route's app bar title must render once pushed"
    );
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

#[test]
fn dialog_add_appends_an_item_and_preserves_home_state() {
    let mut demo = MountedDemo::mount();
    assert_eq!(
        demo.home_create_count(),
        1,
        "MaterialDemoHomeState::create_state must have run exactly once at mount"
    );
    assert_eq!(
        demo.list_item_count(),
        tree::INITIAL_ITEM_COUNT,
        "the list must start with exactly INITIAL_ITEM_COUNT items"
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
    // The list is lazy: the appended row sits below the window and is not
    // built, so its text cannot be found; the declared count is the oracle.
    assert_eq!(
        demo.list_item_count(),
        tree::INITIAL_ITEM_COUNT + 1,
        "Add must append a fresh item (the 21st) to the list"
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

/// (a) An invalid `Name` shows [`tree::NAME_VALIDATION_ERROR`] and disables
/// Submit (a tap while invalid is a no-op); once the name is replaced with a
/// valid one, the error clears and Submit becomes live.
///
/// Red-check: hard-coding `is_valid_name` to always return `true` in
/// `tree.rs` makes the first `find_text(NAME_VALIDATION_ERROR)` assertion
/// fail (the error never shows for "ab"); hard-coding it to always return
/// `false` makes the final `find_text_with_prefix` assertion fail instead
/// (Submit never fires for "Alice"). Both edges of this test are load-bearing.
#[test]
fn invalid_name_shows_the_validation_error_and_disables_submit_until_valid() {
    let mut demo = MountedDemo::mount();
    push_form_route(&mut demo);

    // Too short (2 letters): invalid.
    demo.name_controller.insert_str("ab");
    demo.pump(Duration::ZERO);
    assert!(
        demo.find_text(tree::NAME_VALIDATION_ERROR).is_some(),
        "an under-length name must show the validation error"
    );

    let submit = demo
        .find_text(tree::SUBMIT_LABEL)
        .expect("the Submit button must render");
    demo.tap_node(submit);
    demo.pump(Duration::ZERO);
    assert!(
        demo.find_text_with_prefix(tree::FORM_SUBMITTED_PREFIX)
            .is_none(),
        "Submit must be disabled (a no-op) while the name is invalid"
    );

    // Replace the whole buffer with a valid name.
    let end = demo.name_controller.text().len();
    demo.name_controller.set_selection(0, end);
    demo.name_controller.insert_str("Alice");
    demo.pump(Duration::ZERO);
    assert!(
        demo.find_text(tree::NAME_VALIDATION_ERROR).is_none(),
        "a valid name must clear the validation error"
    );

    let submit = demo
        .find_text(tree::SUBMIT_LABEL)
        .expect("the Submit button must still render");
    demo.tap_node(submit);
    demo.pump(Duration::ZERO);
    assert!(
        demo.find_text(&format!("{}Alice", tree::FORM_SUBMITTED_PREFIX))
            .is_some(),
        "Submit must be enabled once the name is valid, and running it must show the submitted \
         name"
    );
}

/// (b) `Load` → [`tree::LOADING_TEXT`] → [`tree::FETCH_SUCCESS_TEXT`].
///
/// Red-check: deleting `AsyncDriver::poll_ready`'s "never re-poll a task
/// woken during this pump" guard (making a self-waking future resolve
/// within a single frame) collapses the `Loading…` window this test asserts
/// on to zero frames, failing the first assertion below.
#[test]
fn loading_the_async_section_shows_a_spinner_then_the_fetched_data() {
    let mut demo = MountedDemo::mount();
    push_form_route(&mut demo);
    demo.fetch_control.set_should_fail(false);

    let load = demo
        .find_text(tree::LOAD_BUTTON_LABEL)
        .expect("the Load button must render before any attempt");
    demo.tap_node(load);
    demo.pump(Duration::ZERO);

    assert!(
        demo.find_text(tree::LOADING_TEXT).is_some(),
        "tapping Load must show the loading indicator while the fetch is in flight"
    );
    assert!(demo.find_text(tree::LOAD_BUTTON_LABEL).is_none());

    // Two more frame pumps resolve `SimulatedFetch` (its eager subscribe
    // poll, run inline by the tap's own pump above, already spent one).
    demo.pump(Duration::ZERO);
    demo.pump(Duration::ZERO);

    assert!(demo.find_text(tree::LOADING_TEXT).is_none());
    assert!(
        demo.find_text(tree::FETCH_SUCCESS_TEXT).is_some(),
        "the fetch must resolve to its success text"
    );
}

/// (c) `Load` → [`tree::FETCH_ERROR_TEXT`] + Retry → [`tree::FETCH_SUCCESS_TEXT`].
///
/// Red-check: making `Retry`'s handler leave `attempt` unchanged (instead of
/// incrementing it) leaves `FutureBuilder`'s key unchanged, so
/// `did_update_view`'s unchanged-key early return skips resubscribing —
/// the fetch never re-runs and the final success assertion fails.
#[test]
fn a_failed_fetch_shows_the_error_and_retry_recovers() {
    let mut demo = MountedDemo::mount();
    push_form_route(&mut demo);
    demo.fetch_control.set_should_fail(true);

    let load = demo
        .find_text(tree::LOAD_BUTTON_LABEL)
        .expect("the Load button must render");
    demo.tap_node(load);
    demo.pump(Duration::ZERO);
    demo.pump(Duration::ZERO);
    demo.pump(Duration::ZERO);

    assert!(
        demo.find_text(tree::FETCH_ERROR_TEXT).is_some(),
        "a scripted-to-fail fetch must show its error message"
    );
    let retry = demo
        .find_text(tree::RETRY_BUTTON_LABEL)
        .expect("Retry must render once the fetch fails");

    demo.fetch_control.set_should_fail(false);
    demo.tap_node(retry);
    demo.pump(Duration::ZERO);
    assert!(
        demo.find_text(tree::LOADING_TEXT).is_some(),
        "Retry must re-issue the fetch, showing the loading indicator again"
    );

    demo.pump(Duration::ZERO);
    demo.pump(Duration::ZERO);
    assert!(
        demo.find_text(tree::FETCH_SUCCESS_TEXT).is_some(),
        "the retried fetch must succeed once scripted to"
    );
    assert!(demo.find_text(tree::FETCH_ERROR_TEXT).is_none());
}

/// (d) Navigating away while the fetch is in flight cancels it: nothing
/// panics, the cancelled fetch never runs to completion (proven by
/// [`tree::SimulatedFetchControl::delivered_count`] staying `0` across
/// however many more frames it would have needed), and re-pushing the Form
/// route starts from its initial (pre-`Load`) state, not a stale one.
///
/// Red-check: making `FormPage`'s `Load` button re-subscribe through, e.g.,
/// a `Rc<RefCell<Option<TaskToken>>>` outside `FutureBuilder`'s own
/// key/dispose machinery (rather than through `FutureBuilder` itself) would
/// leak the task past the route's disposal — this test's
/// `delivered_count() == 0` assertion is exactly what would catch that: the
/// cancelled-in-isolation `SimulatedFetch` would otherwise complete on one
/// of the trailing pumps below and increment it.
#[test]
fn navigating_away_mid_load_cancels_the_fetch_and_resets_on_return() {
    let mut demo = MountedDemo::mount();
    push_form_route(&mut demo);
    demo.fetch_control.set_should_fail(false);

    let load = demo
        .find_text(tree::LOAD_BUTTON_LABEL)
        .expect("the Load button must render");
    demo.tap_node(load);
    demo.pump(Duration::ZERO);
    assert!(
        demo.find_text(tree::LOADING_TEXT).is_some(),
        "the fetch must still be in flight before navigating away"
    );

    let back_glyph = back_button_glyph_text();
    let back_button = demo
        .find_text(&back_glyph)
        .expect("the Form route's implied BackButton must render");
    demo.tap_node(back_button);
    demo.pump(Duration::ZERO);

    assert!(
        demo.find_text(tree::FORM_ROUTE_TITLE).is_none(),
        "popping the Form route must remove it"
    );
    assert!(
        demo.find_text(tree::APP_TITLE).is_some(),
        "popping must return to the home route"
    );

    // More frames than `SimulatedFetch` would ever need to resolve, had it
    // not been dropped when the route was popped.
    for _ in 0..5 {
        demo.pump(Duration::ZERO);
    }
    assert_eq!(
        demo.fetch_control.delivered_count(),
        0,
        "a cancelled fetch must never run to completion — its future must have been dropped \
         when the route disposed, not merely had its result ignored"
    );

    push_form_route(&mut demo);
    assert!(
        demo.find_text(tree::LOAD_BUTTON_LABEL).is_some(),
        "re-pushing the Form route must start from its initial (pre-Load) state"
    );
    assert!(demo.find_text(tree::LOADING_TEXT).is_none());
    assert!(demo.find_text(tree::FETCH_SUCCESS_TEXT).is_none());
}

// ============================================================================
// `tree.rs` sanity — both `#[path]` consumers reference the same symbols
// ============================================================================

/// `MaterialDemoApp` (the thin `StatelessView` `flui_app::run_app` entry
/// point) is exercised at runtime only by `examples/material_demo/main.rs`.
/// Referencing it here keeps both `#[path]` consumers of `tree.rs` compiling the
/// same symbol set, so a signature change that breaks the example's entry point
/// fails `cargo test` too, not only `cargo build --example`.
#[test]
fn demo_app_entry_point_constructs() {
    let _ = tree::MaterialDemoApp;
}
