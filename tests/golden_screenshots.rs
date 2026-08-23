//! Golden-image regression tests for the demo widget trees.
//!
//! Each test renders a demo headless (via `flui_engine::wgpu::HeadlessRenderer`,
//! the same path as `examples/screenshot.rs`) and compares the pixels against a
//! committed PNG in `tests/goldens/`. A visual regression — a widget that moves,
//! recolors, loses its shadow, or stops rendering — shifts far more than the
//! per-pixel tolerance and fails the matching test.
//!
//! **Gated behind `--features golden`** (see the root `Cargo.toml`): these need a
//! GPU, and the goldens are specific to the machine that generated them (GPU /
//! driver differences move anti-aliased edges), so the normal `cargo nextest`
//! run must not attempt them. Run explicitly on a consistent GPU:
//!
//! ```text
//! cargo nextest run -p flui --features golden --test golden_screenshots
//! UPDATE_GOLDENS=1 cargo nextest run -p flui --features golden --test golden_screenshots  # regenerate
//! ```
//!
//! # What counts as a pass
//!
//! Only a pixel comparison against a committed golden. Both ways this suite
//! could previously go green without comparing anything are now failures:
//!
//! - **A missing golden fails.** Writing the absent PNG and returning would let a
//!   deleted — or never-committed — golden heal itself into a pass, so a golden
//!   is only ever written under `UPDATE_GOLDENS=1`, which is an explicit
//!   regeneration, not a test run.
//! - **A missing GPU fails.** Silently returning on `HeadlessRenderer::new()`
//!   error made "no GPU on this machine" indistinguishable from "the pixels
//!   matched". Set `FLUI_GOLDEN_ALLOW_NO_GPU=1` to opt out loudly on a box that
//!   genuinely has no device; the suite then reports each skip on stderr instead
//!   of pretending to have run.
#![cfg(feature = "golden")]

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

use std::path::PathBuf;

use flui_engine::wgpu::HeadlessRenderer;
use flui_testing::HeadlessBinding;
use flui_testing::bootstrap::{MountOptions, MountOwners};
use flui_view::IntoView;
use flui_widgets::{FocusRoot, GestureArenaScope, VsyncScope};

/// A single channel may differ by up to this much (0–255) before a pixel counts
/// as "changed" — absorbs the sub-pixel jitter same-GPU rendering can still show
/// frame to frame. The goldens are machine-specific (regenerated per reference
/// GPU), so this stays tight enough to catch a small element shifting.
const CHANNEL_TOLERANCE: u8 = 8;

/// At most this fraction of pixels may exceed [`CHANNEL_TOLERANCE`]. Same-GPU
/// renders are near-deterministic, so the floor is low — a moved icon or a
/// dropped shadow clears it easily.
const MAX_CHANGED_FRACTION: f64 = 0.005;

const SHOT_WIDTH: u32 = 900;
const SHOT_HEIGHT: u32 = 760;

fn goldens_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/goldens")
}

/// Whether a device-less machine may skip instead of failing.
///
/// Opt-in and loud: without it, "no GPU" is a failure, because a silent early
/// return is indistinguishable from a passing comparison.
fn allow_no_gpu() -> bool {
    std::env::var_os("FLUI_GOLDEN_ALLOW_NO_GPU").is_some()
}

/// Mount `root_view` headless, drive its bootstrap frame, and rasterize it to
/// tight RGBA8.
///
/// Returns `None` only when there is no GPU *and* [`allow_no_gpu`] is set;
/// otherwise a missing device fails the test.
fn render_demo<V: IntoView + 'static>(root_view: V) -> Option<Vec<u8>> {
    let renderer = match HeadlessRenderer::new() {
        Ok(renderer) => renderer,
        Err(e) => {
            assert!(
                allow_no_gpu(),
                "golden: no GPU for headless capture ({e}). These tests compare real \
                 pixels, so a device-less run is a failure, not a pass — set \
                 FLUI_GOLDEN_ALLOW_NO_GPU=1 to skip explicitly on a machine that has \
                 no device."
            );
            eprintln!("golden: skipping — no GPU ({e}), FLUI_GOLDEN_ALLOW_NO_GPU is set");
            return None;
        }
    };

    let mut binding = HeadlessBinding::new();

    // The presentation scopes the widget layer supplies. Everything below them —
    // capability install, mount ordering, render-root discovery, the
    // layout<->build fixpoint frame, and the lazy-sliver service pass — is
    // `flui-testing`'s canonical bootstrap, the SAME sequence `pump_frame` and
    // the live `draw_frame` run.
    //
    // This suite used to hand-roll that sequence with a bare
    // `PipelineOwner::run_frame`, which never services build-during-layout
    // content: every committed golden of a demo with a `SliverAppBar` captured
    // its delegate child as empty space. Regenerate the goldens after this
    // change.
    let focused_root = FocusRoot::new(root_view);
    let animated_root = VsyncScope::new(binding.vsync().clone(), focused_root);
    let scoped_root = GestureArenaScope::new(binding.arena().clone(), animated_root);

    let mounted = binding.mount_root(
        &scoped_root,
        MountOwners::fresh(),
        MountOptions::tight(SHOT_WIDTH as f32, SHOT_HEIGHT as f32),
    );
    assert!(
        mounted.painted,
        "the bootstrap frame must produce a LayerTree"
    );

    let layer_tree = binding
        .layer_tree()
        .expect("the bootstrap frame committed a layer tree");

    Some(
        renderer
            .render_layer_tree(layer_tree, (SHOT_WIDTH, SHOT_HEIGHT))
            .expect("headless render of the demo layer tree"),
    )
}

/// Compare `actual` RGBA8 against `tests/goldens/<name>.png`.
///
/// Writes the golden (and returns) ONLY under `UPDATE_GOLDENS` — an explicit
/// regeneration. A missing golden otherwise fails, so a deleted or
/// never-committed one cannot heal itself into a pass. With a golden present,
/// fails if more than [`MAX_CHANGED_FRACTION`] of pixels moved past the
/// tolerance.
fn assert_matches_golden(name: &str, actual: &[u8]) {
    let path = goldens_dir().join(format!("{name}.png"));

    if std::env::var_os("UPDATE_GOLDENS").is_some() {
        std::fs::create_dir_all(goldens_dir()).expect("create tests/goldens/");
        image::save_buffer(
            &path,
            actual,
            SHOT_WIDTH,
            SHOT_HEIGHT,
            image::ExtendedColorType::Rgba8,
        )
        .expect("write golden PNG");
        eprintln!("golden {name}: wrote {}", path.display());
        return;
    }

    // Writing an absent golden and returning would let a deleted or
    // never-committed golden heal itself into a pass. A golden is only ever
    // written under an explicit UPDATE_GOLDENS regeneration.
    assert!(
        path.exists(),
        "golden {name}: {} does not exist, so there is nothing to compare against. \
         Generate it deliberately with `just golden-update` (UPDATE_GOLDENS=1) and \
         commit the PNG.",
        path.display(),
    );

    let golden = image::open(&path)
        .unwrap_or_else(|e| panic!("open golden {}: {e}", path.display()))
        .to_rgba8();
    assert_eq!(
        (golden.width(), golden.height()),
        (SHOT_WIDTH, SHOT_HEIGHT),
        "golden {name} has unexpected dimensions",
    );

    let total = (SHOT_WIDTH * SHOT_HEIGHT) as usize;
    let changed = actual
        .as_chunks::<4>()
        .0
        .iter()
        .zip(golden.as_raw().as_chunks::<4>().0.iter())
        .filter(|(a, g)| {
            a.iter()
                .zip(g.iter())
                .any(|(av, gv)| av.abs_diff(*gv) > CHANNEL_TOLERANCE)
        })
        .count();
    let fraction = changed as f64 / total as f64;

    assert!(
        fraction <= MAX_CHANGED_FRACTION,
        "golden {name}: {changed}/{total} pixels ({:.2}%) exceed the channel \
         tolerance {CHANNEL_TOLERANCE} — a visual regression, or regenerate with \
         UPDATE_GOLDENS=1 if intended (max {:.1}%)",
        fraction * 100.0,
        MAX_CHANGED_FRACTION * 100.0,
    );
}

macro_rules! golden_test {
    ($test_name:ident, $golden:literal, $root:expr) => {
        #[test]
        fn $test_name() {
            let Some(pixels) = render_demo($root) else {
                // Only reachable under FLUI_GOLDEN_ALLOW_NO_GPU; `render_demo`
                // has already reported the skip on stderr. Without that opt-in
                // a missing device fails there instead of returning here.
                return;
            };
            assert_matches_golden($golden, &pixels);
        }
    };
}

golden_test!(golden_material, "material", material_demo::MaterialDemoApp);
golden_test!(
    golden_cupertino,
    "cupertino",
    cupertino_demo::CupertinoDemoApp
);
golden_test!(
    golden_vertical_slice,
    "vertical-slice",
    vertical_slice_demo::DemoApp
);
golden_test!(golden_gallery, "gallery", widgets_gallery::Gallery);
golden_test!(golden_colored_box, "colored-box", colored_box_app::App);
golden_test!(golden_text, "text", text_app::App);
