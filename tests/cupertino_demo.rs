//! Acceptance test for the Cupertino sample app — the Catalog.1 Cupertino
//! exit criterion: "`CupertinoTabScaffold` + `CupertinoNavigationBar` + a
//! `CupertinoPageRoute` swipe-back renders and is interactive."
//!
//! `#[path]`-includes the exact tree `examples/cupertino_demo/main.rs` runs
//! (not a duplicate) and mounts it through `flui_testing::HeadlessBinding`'s
//! public surface, mirroring `tests/material_demo.rs`'s identical harness
//! shape (that file explains why each helper below is duplicated rather
//! than shared: neither test crate can see the other's private items).
//!
//! Honesty notes (Definition of Done) — restated from `tree.rs`'s module
//! doc: this proves the named components mount, lay out, and respond to
//! real gesture dispatch (including edge-swipe-back); it inherits every
//! deferral each component's own module docs already name.

#[path = "../examples/cupertino_demo/tree.rs"]
#[expect(
    dead_code,
    reason = "the `App` entry-point wrapper is exercised by `demo_layer_snapshots`; this target mounts the demo root directly"
)]
mod tree;

use std::cell::Cell;
use std::rc::Rc;
use std::time::Duration;

use flui_cupertino::{CupertinoTabController, CupertinoTheme, CupertinoThemeData};
use flui_foundation::RenderId;
use flui_foundation::geometry::{Offset, Size};
use flui_interaction::events::{PointerType, make_down_event, make_up_event};
use flui_rendering::constraints::BoxConstraints;
use flui_rendering::hit_testing::HitTestResult;
use flui_rendering::pipeline::{PipelineCell, PipelineOwner};
use flui_testing::HeadlessBinding;
use flui_testing::bootstrap::{MountOptions, MountOwners};
use flui_widgets::{FocusRoot, GestureArenaScope, MediaQuery, MediaQueryData, VsyncScope};

/// The mounted root's logical width.
const ROOT_WIDTH: f64 = 400.0;
/// The mounted root's logical height.
const ROOT_HEIGHT: f64 = 800.0;

fn root_constraints() -> BoxConstraints {
    BoxConstraints::tight(Size::new(ROOT_WIDTH, ROOT_HEIGHT))
}

/// Everything the test needs to drive and inspect the mounted demo tree.
struct MountedDemo {
    binding: HeadlessBinding,
    pipeline_owner: PipelineCell,
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

    fn tap_down(&self, x: f64, y: f64) {
        self.dispatch_pointer(make_down_event(offset(x, y), PointerType::Mouse));
    }

    fn tap_up(&self, x: f64, y: f64) {
        self.dispatch_pointer(make_up_event(offset(x, y), PointerType::Mouse));
    }

    /// A full tap (down + up) at `(x, y)`.
    fn tap(&self, x: f64, y: f64) {
        self.tap_down(x, y);
        self.tap_up(x, y);
    }

    /// Taps the center of `id`'s rendered box.
    fn tap_node(&self, id: RenderId) {
        let position = self.absolute_position(id);
        self.tap(position.dx + 1.0, position.dy + 1.0);
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
    /// offsets up the render-tree ancestry.
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

// ============================================================================
// (3) The nav bar's leading chevron and the explicit Back button both pop
// ============================================================================

// ============================================================================
// (4) Edge-swipe-back: a real drag from the left edge pops the Details route
// ============================================================================
