//! Acceptance test for the Material sample app — the Catalog.1 Material
//! exit criterion: "A Material sample app (`Scaffold` + `AppBar` +
//! `FloatingActionButton` + a `ListView` of `Card`s + a `Dialog`) renders
//! and is interactive."
//!
//! `#[path]`-includes the exact tree `examples/material_demo/main.rs` runs
//! (not a duplicate) and mounts it through `flui_testing::HeadlessBinding`'s
//! public surface, then drives it the way an app author's fingers would: tap
//! a card, push the settings route from the app bar and pop it with the back
//! button — asserting on the resulting render tree, not merely "no panic".
//!
//! This test lives in the root crate (not `flui-material`'s own `tests/`)
//! for the same reason `tests/vertical_slice_demo.rs` does: it re-bootstraps
//! a headless tree from `flui-view`/`flui-rendering`/`flui-testing`'s public
//! API only, mirroring `HeadlessBinding`'s own documented mount sequence.
//! Every helper below (`tap`/`find_text`/`absolute_position`) is therefore
//! duplicated from `tests/vertical_slice_demo.rs` rather than shared — neither
//! test crate can see the other's private helpers.
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

#[path = "../examples/two_screens/tree.rs"]
mod notes;

#[test]
fn replacing_material_demo_with_notes_keeps_retained_render_nodes_live() {
    use flui::prelude::*;
    use flui::widgets::column;
    use flui_testing::widgets::{lay_out, tight};

    #[derive(Clone, StatefulView)]
    struct Switcher;
    struct SwitcherState {
        notes: Signal<bool>,
    }
    impl StatefulView for Switcher {
        type State = SwitcherState;
        fn create_state(&self) -> Self::State {
            SwitcherState {
                notes: Signal::default(),
            }
        }
    }
    impl ViewState<Switcher> for SwitcherState {
        fn init_state(&mut self, cx: &dyn LifecycleContext) {
            self.notes = cx.signal(false);
        }
        fn build(&self, _: &Switcher, cx: &dyn BuildContext) -> impl IntoView {
            let notes = self.notes;
            let content = if notes.get(cx) {
                notes::NotesApp::default().boxed()
            } else {
                tree::MaterialDemoApp.boxed()
            };
            Column::new(column![
                SizedBox::height(48.0).child(
                    RawButton::new(Text::new("Switch scene"))
                        .on_press(move |cx| notes.set(cx, true))
                ),
                Expanded::new(content),
            ])
        }
    }
    let ((), log) = flui_testing::log_capture::capture(|| {
        let mut laid = lay_out(Switcher, tight(400.0, 800.0));
        let id = laid
            .find_text("Switch scene")
            .expect("public switch button");
        let origin = laid.absolute_offset(id);
        let size = laid.size(id);
        let (x, y) = (origin.dx + size.width / 2.0, origin.dy + size.height / 2.0);
        laid.dispatch_pointer_down(x, y);
        laid.dispatch_pointer_up(x, y);
        laid.pump();
        assert!(
            laid.find_text("FLUI Notes").is_some(),
            "the replacement scene is laid out"
        );
    });
    assert!(
        !log.is_empty(),
        "capture must observe the real mounted path"
    );
    let errors: Vec<_> = log
        .records()
        .iter()
        .filter(|record| record.message.contains("lifecycle hook panicked"))
        .collect();
    assert_eq!(
        errors.len(),
        0,
        "scene replacement must not invalidate surviving elements: {errors:?}"
    );
}

use std::cell::Cell;
use std::rc::Rc;
use std::time::Duration;

use flui_foundation::RenderId;
use flui_foundation::geometry::{Offset, Size};
use flui_interaction::events::{PointerKind, make_down_event, make_up_event};
use flui_material::back_button::back_arrow_icon_data;
use flui_material::{Theme, ThemeData};
use flui_rendering::constraints::BoxConstraints;
use flui_rendering::hit_testing::HitTestResult;
use flui_rendering::pipeline::{PipelineCell, PipelineOwner};
use flui_rendering::testing::inspect;
use flui_testing::HeadlessBinding;
use flui_testing::bootstrap::{MountOptions, MountOwners};
use flui_widgets::{FocusRoot, GestureArenaScope, MediaQuery, MediaQueryData, VsyncScope};

#[expect(
    dead_code,
    reason = "the interactive example's main is not used by acceptance tests"
)]
#[path = "../examples/motion_lab.rs"]
mod motion_lab;

#[test]
fn motion_lab_buttons_drive_independent_property_deadlines_and_reduce() {
    use flui_animation::MotionOverride;
    use flui_layer::Layer;
    use flui_painting::DrawOp;
    use flui_testing::widgets::{LaidOut, lay_out, tight};

    fn tap(laid: &mut LaidOut, label: &str) {
        let id = laid.find_text(label).expect("the demo button is visible");
        let offset = laid.absolute_offset(id);
        let size = laid.size(id);
        let x = offset.dx + size.width / 2.0;
        let y = offset.dy + size.height / 2.0;
        laid.dispatch_pointer_down(x, y);
        laid.dispatch_pointer_up(x, y);
        laid.pump();
    }

    fn moving_box(laid: &LaidOut) -> (f64, f64) {
        for (_, node) in laid.layer_tree().expect("a committed demo scene").iter() {
            let Layer::Picture(picture) = node.layer() else {
                continue;
            };
            for command in picture.picture() {
                if let DrawOp::Rect { rect, paint } = &command.op
                    && paint.color == flui_painting::styling::Color::rgb(35, 94, 180)
                    && rect.height() != 52.0
                {
                    return (rect.width(), rect.height());
                }
            }
        }
        panic!("the demo paints its moving blue rectangle");
    }

    fn indicator_arc(laid: &LaidOut) -> (f64, f64) {
        for command in laid.draw_ops() {
            if let DrawOp::Arc {
                start_angle,
                sweep_angle,
                ..
            } = command.op
            {
                return (start_angle, sweep_angle);
            }
        }
        panic!("the actual demo retains visible loading feedback");
    }

    for preference in [MotionOverride::Full, MotionOverride::Reduce] {
        let mut laid = lay_out(motion_lab::MotionLab, tight(720.0, 760.0));
        laid.set_motion_override(preference);
        laid.pump();
        assert_eq!(moving_box(&laid), (90.0, 40.0));
        tap(&mut laid, "Change width");
        laid.pump_for(Duration::from_millis(16));
        laid.pump_for(Duration::from_millis(400));
        let (width, _) = moving_box(&laid);
        if preference == MotionOverride::Full {
            assert!((90.0..360.0).contains(&width));
        } else {
            assert_eq!(width, 360.0);
        }
        tap(&mut laid, "Change height");
        laid.pump_for(Duration::from_millis(16));
        laid.pump_for(Duration::from_millis(1200));
        let (width, height) = moving_box(&laid);
        assert_eq!(width, 360.0, "height must not restart width's deadline");
        if preference == MotionOverride::Full {
            assert!((40.0..120.0).contains(&height));
        } else {
            assert_eq!(height, 120.0);
        }
        laid.pump_for(Duration::from_millis(500));
        assert_eq!(moving_box(&laid), (360.0, 120.0));
    }

    use flui_interaction::events::{Code, Key, NamedKey};
    use flui_interaction::testing::input::KeyEventBuilder;
    use flui_testing::a11y::Role;
    let mut laid = lay_out(motion_lab::MotionLab, tight(720.0, 760.0));
    laid.set_motion_override(MotionOverride::Full);
    laid.enable_semantics();
    laid.pump_for(Duration::ZERO);
    laid.pump_for(Duration::from_millis(300));
    let tree = laid.a11y_tree().expect("semantics enabled");
    let pause = tree
        .find_by_label("Pause activity")
        .expect("the real demo exposes its pause control");
    assert_eq!(pause.role(), Role::Button);
    assert!(!pause.is_disabled());
    let tab = KeyEventBuilder::new(Code::Tab)
        .with_key(Key::Named(NamedKey::Tab))
        .build();
    let enter = KeyEventBuilder::new(Code::Enter)
        .with_key(Key::Named(NamedKey::Enter))
        .build();
    let mut reached = false;
    for _ in 0..20 {
        assert!(laid.focus_manager().dispatch_key_event(&tab).is_handled());
        laid.tick();
        let tree = laid.a11y_tree().expect("semantics enabled");
        if tree
            .focus()
            .is_some_and(|node| node.label() == Some("Pause activity"))
        {
            reached = true;
            break;
        }
    }
    assert!(reached, "Tab traversal reaches the actual pause button");
    assert!(laid.focus_manager().dispatch_key_event(&enter).is_handled());
    laid.tick();
    let paused = indicator_arc(&laid);
    assert!(paused.1 > 0.0);
    let tree = laid.a11y_tree().expect("semantics enabled");
    assert_eq!(
        tree.focus().and_then(|node| node.label()),
        Some("Resume activity"),
        "changing the action name retains keyboard focus"
    );
    assert_eq!(
        tree.find_by_label("Resume activity")
            .expect("changed control name")
            .role(),
        Role::Button
    );
    assert!(laid.focus_manager().dispatch_key_event(&tab).is_handled());
    laid.tick();
    assert_ne!(
        laid.a11y_tree()
            .expect("semantics enabled")
            .focus()
            .and_then(|node| node.label()),
        Some("Resume activity")
    );
    laid.pump_for(Duration::from_millis(137));
    assert_eq!(
        indicator_arc(&laid),
        paused,
        "pause survives moving focus at a non-cycle-aligned sample"
    );
    tap(&mut laid, "Change width");
    laid.pump_for(Duration::ZERO);
    laid.pump_for(Duration::from_millis(1600));
    assert_eq!(moving_box(&laid), (360.0, 40.0));
    assert_eq!(
        indicator_arc(&laid),
        paused,
        "other controls remain operable without resuming the indicator"
    );
    laid.pump_for(Duration::from_millis(6137));
    assert_eq!(
        indicator_arc(&laid),
        paused,
        "pause survives moving focus and more than five seconds"
    );
    laid.set_motion_override(MotionOverride::Reduce);
    laid.tick();
    tap(&mut laid, "Resume activity");
    laid.pump_for(Duration::from_millis(300));
    assert_eq!(indicator_arc(&laid), paused, "Resume respects Reduce");
    laid.set_motion_override(MotionOverride::Full);
    laid.pump_for(Duration::ZERO);
    let resumed = indicator_arc(&laid);
    laid.pump_for(Duration::from_millis(300));
    assert_ne!(
        indicator_arc(&laid),
        resumed,
        "returning to Full resumes the real indicator"
    );
}

/// The mounted root's logical width — wide enough for a card row.
const ROOT_WIDTH: f64 = 480.0;
/// The mounted root's logical height — tall enough to show several cards but
/// short enough that [`tree::INITIAL_ITEM_COUNT`] cards (at
/// [`tree::ITEM_EXTENT`] each) overflow it, as they do on a phone.
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
        self.dispatch_pointer(
            make_down_event(offset(x, y), PointerKind::Mouse).expect("finite pointer fixture"),
        );
    }

    /// Hit-test at root-local `(x, y)` and dispatch a synthetic pointer-up —
    /// paired with [`tap_down`](Self::tap_down) at the same position, this
    /// completes a tap.
    fn tap_up(&self, x: f64, y: f64) {
        self.dispatch_pointer(
            make_up_event(offset(x, y), PointerKind::Mouse).expect("finite pointer fixture"),
        );
    }

    fn hit_test(&self, position: Offset) -> HitTestResult {
        self.pipeline_owner.with(|owner| {
            let mut result = HitTestResult::new();
            // Every fixture event is a mouse event.
            owner.hit_test(position, PointerKind::Mouse, &mut result);
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

    /// The unique paragraph or icon whose plain-text content is `text`.
    fn find_text(&self, text: &str) -> Option<RenderId> {
        self.pipeline_owner.with(|owner| {
            let mut found = None;
            for (id, _node) in owner.render_tree().iter() {
                let Some(diagnostics) = owner.debug_node_diagnostics(id) else {
                    continue;
                };
                if !matches!(diagnostics.name(), Some("RenderParagraph" | "RenderIcon")) {
                    continue;
                }
                if diagnostics.get_property("text") == Some(text) {
                    assert!(
                        found.is_none(),
                        "multiple text-producing render nodes contain {text:?}"
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
// The app bar action pushes route 2; the implied BackButton pops back
// with home state intact
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
