//! Facade smoke test — App.1 exit evidence that `flui::prelude::*` alone is
//! enough to author a real widget tree and mount it through the headless
//! pipeline, and that `flui::material`/`flui::cupertino` resolve to
//! constructible values through the facade's re-exports.
//!
//! This lives in the root crate's `tests/` (not `flui-widgets`' own tests)
//! because it is exercising the `flui` package's own public surface — the
//! facade re-exports under test only exist on this package. The mount
//! sequence (`mount_root_with_pipeline_owner` → set root constraints → run
//! one frame) mirrors `tests/material_demo.rs` and `tests/vertical_slice_demo.rs`'s
//! own `MountedDemo::mount` helpers, trimmed to the minimum needed to prove a
//! `flui::prelude`-authored tree mounts and lays out — this test is not
//! another acceptance test for a sample app, just a compile-and-mount
//! smoke check for the facade surface itself.

use std::sync::Arc;

use flui::prelude::*;
use flui_binding::HeadlessBinding;
use flui_rendering::constraints::BoxConstraints;
use flui_rendering::pipeline::PipelineOwner;
use flui_types::Size;
use flui_types::geometry::px;
use flui_view::{BuildOwner, ElementTree};
use parking_lot::RwLock;

/// A trivial tree authored entirely off `flui::prelude::*` — the same import
/// shape `src/lib.rs`'s crate-level doc-test demonstrates.
#[derive(Clone, StatelessView)]
struct FacadeSmokeApp;

impl StatelessView for FacadeSmokeApp {
    fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
        Container::new()
            .color(Color::rgb(18, 18, 24))
            .child(Center::new().child(Text::new("flui facade smoke test")))
    }
}

fn root_constraints() -> BoxConstraints {
    BoxConstraints::tight(Size::new(px(320.0), px(240.0)))
}

#[test]
fn prelude_authored_tree_mounts_through_the_headless_pipeline() {
    let binding = HeadlessBinding::new();
    let mut build_owner = BuildOwner::new();
    let mut tree = ElementTree::new();
    let pipeline_owner = Arc::new(RwLock::new(PipelineOwner::new()));

    binding.install_build_capabilities(&mut build_owner);

    binding.enter_owner_scope(|| {
        let root_element = tree.mount_root_with_pipeline_owner(
            &FacadeSmokeApp,
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
            .expect("the mounted facade smoke tree should have a render root");
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
            .expect("bootstrap frame over the facade smoke tree should succeed");
    });
}

#[test]
fn material_and_cupertino_modules_resolve_through_the_facade() {
    let material_theme = flui::material::ThemeData::light();
    assert_eq!(material_theme.brightness(), Brightness::Light);

    let cupertino_theme = flui::cupertino::CupertinoThemeData::new();
    // A fresh theme carries no brightness override (follows the ambient
    // `MediaQuery` instead) and resolves `primary_color` to the documented
    // default (`CupertinoColors::SYSTEM_BLUE`) — both would fail if
    // `flui::cupertino::CupertinoThemeData` were resolving to the wrong
    // type or a stale default, not just "failed to compile".
    assert_eq!(cupertino_theme.brightness(), None);
    assert_eq!(
        cupertino_theme.primary_color(),
        flui::cupertino::CupertinoColor::Dynamic(flui::cupertino::CupertinoColors::SYSTEM_BLUE)
    );
}
