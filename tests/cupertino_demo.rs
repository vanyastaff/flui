//! Acceptance test for the Cupertino sample app — the Catalog.1 Cupertino
//! exit criterion: "`CupertinoTabScaffold` + `CupertinoNavigationBar` + a
//! `CupertinoPageRoute` swipe-back renders and is interactive."
//!
//! `#[path]`-includes the exact tree `examples/cupertino_demo/main.rs` runs
//! (not a duplicate) and mounts it through `flui_binding::HeadlessBinding`'s
//! public surface, mirroring `tests/material_demo.rs`'s identical harness
//! shape (that file explains why each helper below is duplicated rather
//! than shared: neither test crate can see the other's private items).
//!
//! Honesty notes (Definition of Done) — restated from `tree.rs`'s module
//! doc: this proves the named components mount, lay out, and respond to
//! real gesture dispatch (including edge-swipe-back); it inherits every
//! deferral each component's own module docs already name.

#[path = "../examples/cupertino_demo/tree.rs"]
mod tree;

use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant};

use flui_binding::HeadlessBinding;
use flui_cupertino::{CupertinoTabController, CupertinoTheme, CupertinoThemeData};
use flui_foundation::RenderId;
use flui_interaction::events::{PointerType, make_down_event, make_move_event, make_up_event};
use flui_rendering::constraints::BoxConstraints;
use flui_rendering::hit_testing::HitTestResult;
use flui_rendering::pipeline::PipelineOwner;
use flui_types::geometry::px;
use flui_types::{Offset, Size};
use flui_view::{BuildOwner, ElementTree};
use flui_widgets::{MediaQuery, MediaQueryData, VsyncScope};
use parking_lot::RwLock;

/// The mounted root's logical width.
const ROOT_WIDTH: f32 = 400.0;
/// The mounted root's logical height.
const ROOT_HEIGHT: f32 = 800.0;

/// `CupertinoRouteTransitionMixin.kTransitionDuration` (`route.dart`, oracle
/// tag `3.44.0`) — the push transition's duration.
const PUSH_TRANSITION: Duration = Duration::from_millis(500);
/// `_kDroppedSwipePageAnimationDuration` (`route.dart`, oracle tag `3.44.0`)
/// — the back-gesture release's flat pacing.
const SWIPE_RELEASE_DURATION: Duration = Duration::from_millis(350);
/// Per-pump virtual-time step, enough pumps to carry either transition past
/// its end (matching `tests/material_demo.rs`'s identical `+ 2` budget).
const FRAME: Duration = Duration::from_millis(50);
const PUSH_PUMPS: usize = (PUSH_TRANSITION.as_millis() / FRAME.as_millis()) as usize + 2;
const SWIPE_PUMPS: usize = (SWIPE_RELEASE_DURATION.as_millis() / FRAME.as_millis()) as usize + 2;

fn root_constraints() -> BoxConstraints {
    BoxConstraints::tight(Size::new(px(ROOT_WIDTH), px(ROOT_HEIGHT)))
}

/// Everything the test needs to drive and inspect the mounted demo tree.
struct MountedDemo {
    binding: HeadlessBinding,
    pipeline_owner: Arc<RwLock<PipelineOwner>>,
    /// Clone of the mounted [`tree::CupertinoDemoRoot`]'s tab controller —
    /// lets a test assert the active tab index directly rather than only
    /// through rendered content.
    controller: CupertinoTabController,
    /// Clone of the mounted root's Settings counter — the state-retention
    /// proof, same `Rc`-shared-before-mounting pattern
    /// `tests/material_demo.rs::MountedDemo::home_create_count` uses.
    settings_count: Rc<Cell<u32>>,
}

impl MountedDemo {
    /// Mount `tree::demo_root()` wrapped exactly as
    /// [`tree::CupertinoDemoApp::build`] wraps it (`MediaQuery(default) ->
    /// CupertinoTheme(default)`), run the bootstrap frame, then hand the
    /// owners to `binding`. Mirrors `tests/material_demo.rs::MountedDemo::mount`.
    fn mount() -> Self {
        let mut binding = HeadlessBinding::new();

        let root_view = tree::demo_root();
        let controller = root_view.controller.clone();
        let settings_count = Rc::clone(&root_view.settings_count);
        let wrapped_root = MediaQuery::new(
            MediaQueryData::default(),
            CupertinoTheme::new(CupertinoThemeData::default(), root_view),
        );

        let mut build_owner = BuildOwner::new();
        let mut tree = ElementTree::new();
        let pipeline_owner = Arc::new(RwLock::new(PipelineOwner::new()));

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
            controller,
            settings_count,
        }
    }

    fn active_tab(&self) -> usize {
        self.controller.index()
    }

    fn settings_count(&self) -> u32 {
        self.settings_count.get()
    }

    /// Drive one deterministic frame.
    fn pump(&mut self, dt: Duration) {
        self.binding.pump_frame(dt);
    }

    fn dispatch_at(&self, event: flui_interaction::PointerEvent, x: f32, y: f32) {
        let position = offset(x, y);
        let owner = self.pipeline_owner.read();
        let mut result = HitTestResult::new();
        owner.hit_test(position, &mut result);
        self.binding.enter_owner_scope(|| result.dispatch(&event));
    }

    fn tap_down(&self, x: f32, y: f32) {
        self.dispatch_at(make_down_event(offset(x, y), PointerType::Mouse), x, y);
    }

    fn tap_up(&self, x: f32, y: f32) {
        self.dispatch_at(make_up_event(offset(x, y), PointerType::Mouse), x, y);
    }

    /// A full tap (down + up) at `(x, y)`.
    fn tap(&self, x: f32, y: f32) {
        self.tap_down(x, y);
        self.tap_up(x, y);
    }

    /// Taps the center of `id`'s rendered box.
    fn tap_node(&self, id: RenderId) {
        let position = self.absolute_position(id);
        self.tap(position.dx.get() + 1.0, position.dy.get() + 1.0);
    }

    /// Hit-tests `(x, y)` and dispatches a synthetic pointer-down, returning
    /// the resolved [`HitTestResult`] so the rest of the gesture can be
    /// routed through the **same** targets — real pointer input captures the
    /// hit-test path at down and routes every subsequent move/up for that
    /// pointer through it regardless of where the pointer travels
    /// afterward (`flui_app::AppBinding::handle_input`'s own
    /// `GestureBinding::handle_pointer_event` only re-hit-tests on a fresh
    /// pointer). A **fresh** hit-test per move (as
    /// `tests/material_demo.rs`'s own `drag_move` helper does) only happens
    /// to work there because that test's drag stays within one wide
    /// scrollable's bounds the whole time; an edge-swipe-back gesture
    /// starts inside a 20px-wide strip (`BACK_GESTURE_WIDTH`) and moves the
    /// pointer far outside it, so re-hit-testing each move would silently
    /// stop reaching the detector partway through the drag.
    fn begin_drag(&self, x: f32, y: f32) -> HitTestResult {
        advance_gesture_clock();
        let position = offset(x, y);
        let mut result = HitTestResult::new();
        {
            let owner = self.pipeline_owner.read();
            owner.hit_test(position, &mut result);
        }
        self.binding
            .enter_owner_scope(|| result.dispatch(&make_down_event(position, PointerType::Mouse)));
        result
    }

    /// Dispatches a synthetic pointer-move at `(x, y)` through `result` (the
    /// [`HitTestResult`] [`begin_drag`](Self::begin_drag) captured), not a
    /// fresh hit-test — see that method's doc.
    fn continue_drag(&self, result: &HitTestResult, x: f32, y: f32) {
        advance_gesture_clock();
        let event = make_move_event(offset(x, y), PointerType::Mouse);
        self.binding.enter_owner_scope(|| result.dispatch(&event));
    }

    /// Dispatches a synthetic pointer-up at `(x, y)` through `result`,
    /// completing the drag started by [`begin_drag`](Self::begin_drag).
    fn end_drag(&self, result: &HitTestResult, x: f32, y: f32) {
        let event = make_up_event(offset(x, y), PointerType::Mouse);
        self.binding.enter_owner_scope(|| result.dispatch(&event));
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

    /// Every render node whose short type name equals `render_type_name`.
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

    /// The string value of a mounted render node's named diagnostic
    /// property.
    fn render_property(&self, id: RenderId, property_name: &str) -> Option<String> {
        let owner = self.pipeline_owner.read();
        let diagnostics = owner.debug_node_diagnostics(id)?;
        diagnostics.get_property(property_name).map(str::to_string)
    }

    /// The screen-space (root-local) top-left of `id`, by summing paint
    /// offsets up the render-tree ancestry.
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
/// returned by the immediately preceding call. Mirrors
/// `tests/material_demo.rs`'s identical helper: `DragGestureRecognizer`
/// timestamps every velocity-tracker sample with `Instant::now()`, and two
/// dispatches landing in the same OS timer tick make the least-squares
/// velocity fit singular. Calling this before each drag dispatch guarantees
/// consecutive samples get strictly increasing timestamps.
fn advance_gesture_clock() {
    let t0 = Instant::now();
    while Instant::now() == t0 {
        std::hint::spin_loop();
    }
}

/// The primary `SlideTransition`'s `RenderFractionalTranslation` — whichever
/// of the two the details route's `cupertino_page_transitions` mounts reads
/// the larger `|dx|` at any given moment (the secondary never moves for a
/// route nothing covers) — see `flui-cupertino/tests/route.rs`'s identical
/// helper and its doc for why this, not raw layout offset, is the right
/// probe for a paint-time transform.
fn primary_slide_dx(demo: &MountedDemo) -> f32 {
    let nodes = demo.find_all_by_render_type("RenderFractionalTranslation");
    assert_eq!(
        nodes.len(),
        2,
        "the primary and secondary SlideTransition each mount one FractionalTranslation"
    );
    nodes
        .into_iter()
        .map(|id| {
            let property = demo
                .render_property(id, "translation")
                .expect("FractionalTranslation always reports its translation");
            let trimmed = property.trim_matches(['(', ')']);
            let dx: f32 = trimmed
                .split(", ")
                .next()
                .expect("translation has a dx component")
                .parse()
                .expect("dx is a float");
            dx
        })
        .fold(0.0_f32, |largest, dx| {
            if dx.abs() > largest.abs() {
                dx
            } else {
                largest
            }
        })
}

// ============================================================================
// (1) Both tabs mount; switching tabs preserves the Settings counter
// ============================================================================

#[test]
fn tabs_mount_and_switching_preserves_the_settings_counter() {
    let mut demo = MountedDemo::mount();
    assert_eq!(demo.active_tab(), 0, "Home is active by default");

    demo.find_text(tree::HOME_NAV_TITLE)
        .expect("the Home tab's nav bar title must render on mount");
    demo.find_text(tree::PUSH_BUTTON_LABEL)
        .expect("the Home tab's push button must render on mount");

    let settings_tab_label = demo
        .find_text(tree::SETTINGS_TAB_LABEL)
        .expect("the Settings tab bar item's label must render");
    demo.tap_node(settings_tab_label);
    demo.pump(Duration::ZERO);
    assert_eq!(
        demo.active_tab(),
        1,
        "tapping the Settings item must switch tabs"
    );

    demo.find_text(tree::SETTINGS_NAV_TITLE)
        .expect("the Settings tab's own nav bar title must render once active");
    assert_eq!(demo.settings_count(), 0);

    let increment = demo
        .find_text(tree::INCREMENT_BUTTON_LABEL)
        .expect("the Settings tab's Increment button must render");
    demo.tap_node(increment);
    demo.pump(Duration::ZERO);
    demo.tap_node(increment);
    demo.pump(Duration::ZERO);
    assert_eq!(
        demo.settings_count(),
        2,
        "two taps must advance the counter to 2"
    );

    // Switch back to Home, then back to Settings — the counter must not reset.
    let home_tab_label = demo
        .find_text(tree::HOME_TAB_LABEL)
        .expect("the Home tab bar item's label must render");
    demo.tap_node(home_tab_label);
    demo.pump(Duration::ZERO);
    assert_eq!(demo.active_tab(), 0);
    demo.find_text(tree::HOME_NAV_TITLE)
        .expect("Home's content must still render after switching back to it");

    let settings_tab_label = demo
        .find_text(tree::SETTINGS_TAB_LABEL)
        .expect("the Settings tab bar item must still render from the Home tab");
    demo.tap_node(settings_tab_label);
    demo.pump(Duration::ZERO);
    assert_eq!(demo.active_tab(), 1);
    assert_eq!(
        demo.settings_count(),
        2,
        "switching away to Home and back to Settings must not reset the counter — \
         CupertinoTabScaffold keeps an inactive tab's state alive via Offstage, not unmount"
    );
}

// ============================================================================
// (2) Pushing Details actually slides the page in over the 500ms transition
// ============================================================================

#[test]
fn pushing_details_slides_the_page_in_over_the_full_transition() {
    let mut demo = MountedDemo::mount();

    assert!(
        demo.find_text(tree::DETAILS_ROUTE_TEXT).is_none(),
        "the Details route must not be built before it is pushed"
    );

    let push_button = demo
        .find_text(tree::PUSH_BUTTON_LABEL)
        .expect("the Home tab's push button must render");
    demo.tap_node(push_button);
    demo.pump(Duration::ZERO);

    demo.find_text(tree::DETAILS_ROUTE_TEXT)
        .expect("the Details route's body text must render once pushed");
    demo.find_text(tree::DETAILS_NAV_TITLE)
        .expect("the Details route's nav bar title must render once pushed");

    let start_dx = primary_slide_dx(&demo);
    assert!(
        (start_dx - 1.0).abs() < 0.01,
        "the pushed page must start fully off-screen to the right (dx == 1.0): {start_dx}"
    );

    demo.pump(PUSH_TRANSITION / 2);
    let midpoint_dx = primary_slide_dx(&demo);
    assert!(
        (0.05..0.95).contains(&midpoint_dx),
        "the page must still be sliding at the transition's midpoint: {midpoint_dx}"
    );

    for _ in 0..PUSH_PUMPS {
        demo.pump(FRAME);
    }
    let settled_dx = primary_slide_dx(&demo);
    assert!(
        settled_dx.abs() < 0.01,
        "the page must settle flush with the viewport once the transition completes: \
         {settled_dx}"
    );
}

// ============================================================================
// (3) The nav bar's leading chevron and the explicit Back button both pop
// ============================================================================

#[test]
fn nav_bar_leading_button_pops_the_details_route() {
    let mut demo = MountedDemo::mount();

    let push_button = demo.find_text(tree::PUSH_BUTTON_LABEL).unwrap();
    demo.tap_node(push_button);
    for _ in 0..PUSH_PUMPS {
        demo.pump(FRAME);
    }
    demo.find_text(tree::DETAILS_ROUTE_TEXT)
        .expect("the Details route must be showing before popping it");

    let nav_back = demo
        .find_text(tree::NAV_BACK_LABEL)
        .expect("the Details route's nav bar leading button must render");
    demo.tap_node(nav_back);
    for _ in 0..PUSH_PUMPS {
        demo.pump(FRAME);
    }

    assert!(
        demo.find_text(tree::DETAILS_ROUTE_TEXT).is_none(),
        "the nav bar's leading button must pop the Details route"
    );
    demo.find_text(tree::PUSH_BUTTON_LABEL)
        .expect("the Home tab's content must still render once popped back to it");
}

#[test]
fn explicit_back_button_pops_the_details_route() {
    let mut demo = MountedDemo::mount();

    let push_button = demo.find_text(tree::PUSH_BUTTON_LABEL).unwrap();
    demo.tap_node(push_button);
    for _ in 0..PUSH_PUMPS {
        demo.pump(FRAME);
    }

    let back_button = demo
        .find_text(tree::BACK_BUTTON_LABEL)
        .expect("the Details route's explicit Back button must render");
    demo.tap_node(back_button);
    for _ in 0..PUSH_PUMPS {
        demo.pump(FRAME);
    }

    assert!(
        demo.find_text(tree::DETAILS_ROUTE_TEXT).is_none(),
        "the explicit Back button must pop the Details route"
    );
}

// ============================================================================
// (4) Edge-swipe-back: a real drag from the left edge pops the Details route
// ============================================================================

#[test]
fn edge_swipe_from_the_left_pops_the_details_route() {
    let mut demo = MountedDemo::mount();

    let push_button = demo.find_text(tree::PUSH_BUTTON_LABEL).unwrap();
    demo.tap_node(push_button);
    // Let the push transition fully settle first — `back_gesture.rs`'s edge
    // detector is mounted throughout, but the swipe itself should start from
    // a page that has actually arrived, not mid-entrance.
    for _ in 0..PUSH_PUMPS {
        demo.pump(FRAME);
    }
    demo.find_text(tree::DETAILS_ROUTE_TEXT)
        .expect("the Details route must be showing before swiping it back");

    // A monotonically rightward drag from inside the 20px edge region, well
    // past the halfway point of the 400px-wide root — both the release
    // position (< 0.5) and any fling velocity reading (positive, i.e. in the
    // pop direction, since every sample moves further right than the last)
    // agree on "pop", so the outcome does not depend on exactly how large a
    // velocity the real-clock-timed samples happen to produce.
    let drag = demo.begin_drag(5.0, 400.0);
    demo.continue_drag(&drag, 60.0, 400.0);
    demo.continue_drag(&drag, 140.0, 400.0);
    demo.continue_drag(&drag, 220.0, 400.0);
    demo.continue_drag(&drag, 300.0, 400.0);
    demo.end_drag(&drag, 300.0, 400.0);

    for _ in 0..SWIPE_PUMPS {
        demo.pump(FRAME);
    }

    assert!(
        demo.find_text(tree::DETAILS_ROUTE_TEXT).is_none(),
        "an edge swipe past the halfway point must pop the Details route"
    );
    demo.find_text(tree::PUSH_BUTTON_LABEL)
        .expect("the Home tab's content must render again once swiped back to it");
}

#[test]
fn demo_app_entry_point_constructs() {
    let _ = tree::CupertinoDemoApp;
}
