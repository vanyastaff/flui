//! Facade smoke test — evidence that `flui::prelude::*` alone is enough to
//! author a real widget tree and mount it through the headless pipeline, and
//! that each feature-selected catalog resolves to constructible values through
//! the facade's re-exports.
//!
//! The first test carries **no** catalog requirement on purpose: it is the
//! `--no-default-features` evidence that a catalog-free application still gets
//! the full widget layer. The remaining tests are `#[cfg]`-gated per feature,
//! so every supported combination compiles and runs exactly the assertions its
//! surface supports — a combination that silently lost a module fails to
//! compile rather than quietly testing less.
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

use flui::prelude::*;
use flui_foundation::geometry::Size;
use flui_rendering::constraints::BoxConstraints;
use flui_testing::HeadlessBinding;
use flui_testing::bootstrap::{MountOptions, MountOwners};

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
    BoxConstraints::tight(Size::new(320.0, 240.0))
}

#[test]
fn prelude_authored_tree_mounts_through_the_headless_pipeline() {
    // Through `flui-testing`'s canonical bootstrap rather than a hand-rolled
    // copy of it: the ordering is load-bearing at nearly every step, and copies
    // of it have drifted silently before.
    let mut binding = HeadlessBinding::new();
    let mounted = binding.mount_root(
        &FacadeSmokeApp,
        MountOwners::fresh(),
        MountOptions::new(root_constraints()),
    );
    assert!(
        mounted.painted,
        "a prelude-authored tree must commit a frame through the headless pipeline"
    );
}

#[test]
fn a_fixed_oval_clip_is_configured_through_the_facade() {
    use flui::geometry::{RRect, Rect};
    use flui::widgets::{ClipOval, Oval};
    use flui_testing::widgets::{lay_out, tight};

    let bounds = Rect::from_ltrb(20.0, 30.0, 80.0, 70.0);
    let tree = lay_out(
        ClipOval::new()
            .clipper(Oval::from_rect(bounds))
            .child(ColoredBox::new(Color::rgb(20, 40, 60))),
        tight(100.0, 100.0),
    );

    // The fixed ellipse is offset from and smaller than the allocated box, so
    // neither a bounds-derived oval nor a rectangular clip produces this layer.
    assert_eq!(
        tree.clip_rrect_layers(),
        vec![RRect::from_rect_elliptical(bounds, 30.0, 20.0)],
    );
}
