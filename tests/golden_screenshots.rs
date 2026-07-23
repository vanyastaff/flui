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
use std::sync::Arc;

use flui_binding::HeadlessBinding;
use flui_engine::wgpu::HeadlessRenderer;
use flui_rendering::constraints::BoxConstraints;
use flui_rendering::pipeline::PipelineOwner;
use flui_types::Size;
use flui_types::geometry::px;
use flui_view::{BuildOwner, ElementTree, IntoView};
use flui_widgets::VsyncScope;
use parking_lot::RwLock;

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

/// Mount `root_view` headless, drive one frame, and rasterize it to tight RGBA8.
/// Mirrors `examples/screenshot.rs`; returns `None` when no GPU is available so
/// the tests skip rather than fail on a headless-CI machine.
fn render_demo<V: IntoView + 'static>(root_view: V) -> Option<Vec<u8>> {
    let renderer = match HeadlessRenderer::new() {
        Ok(renderer) => renderer,
        Err(e) => {
            eprintln!("skipping golden test: no GPU for headless capture ({e})");
            return None;
        }
    };

    let binding = HeadlessBinding::new();
    let mut build_owner = BuildOwner::new();
    let mut element_tree = ElementTree::new();
    let pipeline_owner = Arc::new(RwLock::new(PipelineOwner::new()));
    binding.install_build_capabilities(&mut build_owner);

    let scoped_root = VsyncScope::new(binding.vsync().clone(), root_view);
    binding.enter_owner_scope(|| {
        let root_element = element_tree.mount_root_with_pipeline_owner(
            &scoped_root,
            Some(Arc::clone(&pipeline_owner)),
            &mut build_owner.element_owner_mut(),
        );
        build_owner.schedule_build_for(root_element, 0, flui_view::RebuildReason::InitialMount);
        build_owner.build_scope(&mut element_tree);
    });

    let root_render_id = {
        let owner = pipeline_owner.read();
        let render_tree = owner.render_tree();
        render_tree
            .iter()
            .map(|(id, _)| id)
            .find(|id| render_tree.parent(*id).is_none())
            .expect("the mounted demo tree must have a render root")
    };
    {
        let mut guard = pipeline_owner.write();
        guard.set_root_id(Some(root_render_id));
        guard.set_root_constraints(Some(BoxConstraints::tight(Size::new(
            px(SHOT_WIDTH as f32),
            px(SHOT_HEIGHT as f32),
        ))));
    }

    let layer_tree = binding.enter_owner_scope(|| {
        let mut guard = pipeline_owner.write();
        let owner = std::mem::take(&mut *guard);
        let (owner, result) = owner.run_frame();
        *guard = owner;
        result.expect("the render frame must succeed")
    });
    let layer_tree = layer_tree.expect("the render frame must produce a LayerTree");

    Some(
        renderer
            .render_layer_tree(&layer_tree, (SHOT_WIDTH, SHOT_HEIGHT))
            .expect("headless render of the demo layer tree"),
    )
}

/// Compare `actual` RGBA8 against `tests/goldens/<name>.png`. Writes the golden
/// (and returns) when it is missing or `UPDATE_GOLDENS` is set; otherwise fails
/// if more than [`MAX_CHANGED_FRACTION`] of pixels moved past the tolerance.
fn assert_matches_golden(name: &str, actual: &[u8]) {
    let path = goldens_dir().join(format!("{name}.png"));
    let updating = std::env::var_os("UPDATE_GOLDENS").is_some();

    if updating || !path.exists() {
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
        .chunks_exact(4)
        .zip(golden.as_raw().chunks_exact(4))
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
                return; // no GPU on this machine — skip
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
