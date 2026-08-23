//! Structural snapshots of the demo widget trees' painted output.
//!
//! Each demo is mounted headless at a fixed size and its committed
//! `LayerTree` — the layer structure plus every draw command, with geometry,
//! colors, transforms, and text — is serialized to text and compared against a
//! committed `insta` snapshot. A visual regression that matters structurally
//! (a widget that moves or resizes, a shadow that stops being emitted, a clip
//! that disappears, a subtree that stops being built) changes those lines and
//! fails the matching test, naming the layer and the command rather than a
//! pixel count.
//!
//! # Why structure and not pixels
//!
//! This suite replaces a pixel-golden suite that compared the same demos
//! against committed PNGs. That suite ran on no CI job and could not be moved
//! onto one: its baselines were bound to the machine that captured them.
//! Measured, the binding was not where its own documentation claimed. Against
//! a completely different rasterizer (llvmpipe vs. the reference device):
//!
//! - the flat-fill demo was **bit-identical**, 0 of 684 000 pixels differing;
//! - the vector demo differed on 0.20% of pixels, all anti-aliased edges;
//! - the four text-carrying demos differed on 0.52%–0.76% — against a 0.5%
//!   failure threshold — and every differing pixel was a glyph, or a widget
//!   sized to one (a button hugging its label moves its whole rectangle). The
//!   same string measured 24 px (9.4%) wider with a 2 px baseline shift, which
//!   is a different font face, not a different rasterizer.
//!
//! So the pixel suite's real baseline was the host's *font installation*, and
//! its pass/fail line sat inside its own cross-machine noise. What remains
//! genuinely pixel-shaped — blending, filters, gradients, glyph raster — is
//! covered per primitive by flui-engine's readback/oracle suite on WARP
//! (merge-blocking), and the fact that a real window presents at all by
//! `tools/live-smoke`. This suite covers the band neither of those reaches:
//! a whole composed demo tree's layout and paint.
//!
//! # Determinism
//!
//! Text measurement resolves against the *host's* fonts, and widgets sized to
//! their text inherit that: measured, the Cupertino demo's button came out
//! 61.18 px wide on a host with fonts installed and 129.55 px on one without.
//! [`pin_font_faces`] builds the process-wide font system from the faces this
//! repository ships, once per process, so the committed geometry is
//! reproducible off this machine. Everything else in the serialized form is
//! documented stable: two-decimal floats, insertion-ordered children, no
//! hash-map iteration.
//!
//! ```text
//! just demo-snapshots                     # run
//! cargo insta review                      # review + accept intended changes
//! ```

#![cfg(all(feature = "material", feature = "cupertino"))]

#[allow(dead_code, unused_imports)]
#[path = "../examples/colored_box_app.rs"]
mod colored_box_app;
#[path = "../examples/cupertino_demo/tree.rs"]
mod cupertino_demo;
#[path = "../examples/material_demo/tree.rs"]
mod material_demo;
#[allow(dead_code, unused_imports)]
#[path = "../examples/text_app.rs"]
mod text_app;
#[path = "../examples/vertical_slice_demo/tree.rs"]
mod vertical_slice_demo;
#[allow(dead_code, unused_imports)]
#[path = "../examples/widgets_gallery.rs"]
mod widgets_gallery;

use std::sync::Once;

use flui_rendering::testing::{collect_commands, serialize_layer_tree};
use flui_testing::HeadlessBinding;
use flui_testing::bootstrap::{MountOptions, MountOwners};
use flui_testing::fonts::pin_font_faces;
use flui_view::IntoView;
use flui_widgets::{FocusRoot, GestureArenaScope, VsyncScope};

/// The mounted size every demo is snapshotted at. Wide and tall enough that
/// each demo's list overflows, so a scrollable's clip and its off-screen
/// children are part of what the snapshot pins.
const SHOT_WIDTH: f32 = 900.0;
const SHOT_HEIGHT: f32 = 760.0;

/// Pins the shared font database to the faces this repository ships.
///
/// Once per process: `cosmic-text` caches shaping per `FontSystem`, so the
/// face set has to be settled before the first measurement, and every test in
/// this binary shares that process.
fn pin_fonts() {
    static PIN: Once = Once::new();
    PIN.call_once(|| {
        pin_font_faces(
            &[
                flui_engine::fonts::ROBOTO_REGULAR,
                flui_engine::fonts::MATERIAL_ICONS_REGULAR,
                flui_engine::fonts::CUPERTINO_ICONS,
            ],
            "Roboto",
        );
    });
}

/// Mounts `root_view` headless and serializes the layer tree its bootstrap
/// frame commits.
///
/// The mount goes through `flui-testing`'s canonical bootstrap — the same
/// sequence `pump_frame` and the live `draw_frame` run, including the
/// layout↔build fixpoint and the lazy-sliver service pass — so a demo whose
/// content is built during layout is snapshotted built, not empty.
fn snapshot_of<V: IntoView + 'static>(root_view: V, min_commands: usize) -> String {
    pin_fonts();

    let mut binding = HeadlessBinding::new();

    // The presentation scopes the widget layer supplies; everything below them
    // belongs to the bootstrap.
    let focused_root = FocusRoot::new(root_view);
    let animated_root = VsyncScope::new(binding.vsync().clone(), focused_root);
    let scoped_root = GestureArenaScope::new(binding.arena().clone(), animated_root);

    let mounted = binding.mount_root(
        &scoped_root,
        MountOwners::fresh(),
        MountOptions::tight(SHOT_WIDTH, SHOT_HEIGHT),
    );
    assert!(
        mounted.painted,
        "the bootstrap frame must commit a layer tree"
    );

    let layer_tree = binding
        .layer_tree()
        .expect("the bootstrap frame committed a layer tree");

    let commands = collect_commands(layer_tree).len();
    assert!(
        commands >= min_commands,
        "the demo painted {commands} draw commands, fewer than its {min_commands} \
         floor — the tree collapsed rather than rendering, so accepting a snapshot \
         of it would commit the breakage as an intended change",
    );

    serialize_layer_tree(layer_tree)
}

/// `snapshot_test!(test, "snapshot-name", root_view, min_draw_commands)`.
///
/// The floor is a collapse guard, not a measurement: it sits roughly half way
/// below what the demo paints today, so a tree that stops rendering fails with
/// a count instead of being accepted into the snapshot as an intended change,
/// while an ordinary edit that adds or removes a few commands never trips it.
macro_rules! snapshot_test {
    ($test_name:ident, $snapshot:literal, $root:expr, $min_commands:expr) => {
        #[test]
        fn $test_name() {
            insta::assert_snapshot!($snapshot, snapshot_of($root, $min_commands));
        }
    };
}

snapshot_test!(
    material_demo,
    "material",
    material_demo::MaterialDemoApp,
    30
);
snapshot_test!(
    cupertino_demo,
    "cupertino",
    cupertino_demo::CupertinoDemoApp,
    8
);
snapshot_test!(
    vertical_slice_demo,
    "vertical-slice",
    vertical_slice_demo::DemoApp,
    15
);
snapshot_test!(widgets_gallery, "gallery", widgets_gallery::Gallery, 5);
// The two single-command demos: their floor IS their content.
snapshot_test!(colored_box, "colored-box", colored_box_app::App, 1);
snapshot_test!(text, "text", text_app::App, 1);
