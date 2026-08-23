//! Headless screenshot of a demo widget tree — no window.
//!
//! Mounts a chosen demo's exact widget tree through `HeadlessBinding`, drives
//! one frame to a `LayerTree`, rasterizes it to an offscreen GPU texture via
//! [`flui_engine::wgpu::HeadlessRenderer`], reads the pixels back, and writes a
//! PNG. This is the capture path OS screenshot tools cannot provide on a
//! GNOME/Wayland session (the wgpu surface never lands in the X11 framebuffer).
//!
//! Run: `cargo run -p flui --example screenshot -- <demo> [width] [height] [out.png]`
//! where `<demo>` is `material` | `cupertino` | `vertical-slice` | `gallery` |
//! `animated-box` | `colored-box` | `text` | `telemetry-overlay`.
//! Defaults: `material`, 900 x 760, `<demo>.png`.
//!
//! Captures one frame at mount time (t=0): animated examples show their initial
//! state, not mid-animation.
//!
//! The mount sequence mirrors `tests/vertical_slice_demo.rs` (mount root →
//! attach `PipelineOwner` → set root constraints); the LayerTree extraction
//! mirrors `RendererBinding::draw_frame`.

// The demos' trees are `#[path]`-included — the exact roots `run_app` mounts on
// screen. Multi-file demos expose a `tree.rs` (no `fn main`). Single-file
// examples are pulled in whole; their own `fn main` (and its `run_app` imports)
// become dead module items here, hence the `allow`.
#[path = "cupertino_demo/tree.rs"]
mod cupertino_demo;
#[path = "material_demo/tree.rs"]
mod material_demo;
#[path = "vertical_slice_demo/tree.rs"]
mod vertical_slice_demo;

#[allow(dead_code, unused_imports)]
#[path = "animated_box_app.rs"]
mod animated_box_app;
#[allow(dead_code, unused_imports)]
#[path = "colored_box_app.rs"]
mod colored_box_app;
#[allow(dead_code, unused_imports)]
#[path = "sliver_demo.rs"]
mod sliver_demo_app;
#[allow(dead_code, unused_imports)]
#[path = "text_app.rs"]
mod text_app;
#[allow(dead_code, unused_imports)]
#[path = "widgets_gallery.rs"]
mod widgets_gallery;

use flui_engine::wgpu::HeadlessRenderer;
use flui_layer::{Layer, LayerTree, PerformanceOverlayLayer};
use flui_testing::HeadlessBinding;
use flui_testing::bootstrap::{MountOptions, MountOwners};
use flui_view::IntoView;
use flui_widgets::{FocusRoot, GestureArenaScope, VsyncScope};

fn main() {
    let mut args = std::env::args().skip(1);
    let demo = args.next().unwrap_or_else(|| "material".to_string());
    let width: u32 = args.next().and_then(|s| s.parse().ok()).unwrap_or(900);
    let height: u32 = args.next().and_then(|s| s.parse().ok()).unwrap_or(760);
    let out_path = args.next().unwrap_or_else(|| format!("{demo}.png"));

    let renderer = HeadlessRenderer::new().expect("a GPU device for headless capture");
    // A mounted demo's `LayerTree` is owned by the binding that produced it
    // (`LayerTree` is not `Clone`), so rasterization happens inside each arm
    // rather than after the match — the binding stays alive exactly as long as
    // the tree being rendered.
    let raster = |layer_tree: &LayerTree| {
        renderer
            .render_layer_tree(layer_tree, (width, height))
            .expect("headless render of the demo layer tree")
    };

    let rgba = match demo.as_str() {
        "material" => capture(material_demo::MaterialDemoApp, width, height, &raster),
        "cupertino" => capture(cupertino_demo::CupertinoDemoApp, width, height, &raster),
        "vertical-slice" | "vslice" => {
            capture(vertical_slice_demo::DemoApp, width, height, &raster)
        }
        "gallery" => capture(widgets_gallery::Gallery, width, height, &raster),
        "animated-box" => capture(animated_box_app::App::new(), width, height, &raster),
        "colored-box" => capture(colored_box_app::App, width, height, &raster),
        "text" => capture(text_app::App, width, height, &raster),
        "telemetry-overlay" => raster(&telemetry_overlay_layers()),
        // The collapsing-sliver scene at three scroll depths — a visual
        // check on the SliverAppBar / FlexibleSpaceBar / pinned-header
        // pipeline (expanded, mid-collapse with the background fading and
        // parallaxing, and fully collapsed to the pinned toolbar).
        "sliver" => capture(sliver_demo_app::tree(0.0), width, height, &raster),
        "sliver-mid" => capture(sliver_demo_app::tree(90.0), width, height, &raster),
        "sliver-collapsed" => capture(sliver_demo_app::tree(500.0), width, height, &raster),
        other => {
            eprintln!(
                "unknown demo {other:?}; expected: material | cupertino | vertical-slice | \
                 gallery | animated-box | colored-box | text | telemetry-overlay | \
                 sliver | sliver-mid | sliver-collapsed"
            );
            std::process::exit(2);
        }
    };

    image::save_buffer(
        &out_path,
        &rgba,
        width,
        height,
        image::ExtendedColorType::Rgba8,
    )
    .expect("encode the captured pixels as PNG");

    println!("wrote {out_path} ({demo}, {width}x{height})");
}

fn telemetry_overlay_layers() -> LayerTree {
    let mut overlay = PerformanceOverlayLayer::all_stats(PerformanceOverlayLayer::default_bounds());
    overlay.set_diagnostic_line(Some(
        "present_p99=16ms input_p99=24ms deferred=3 dropped=1 input_truncated=false".to_string(),
    ));

    let mut tree = LayerTree::new();
    let root = tree.insert(Layer::PerformanceOverlay(Box::new(overlay)));
    tree.set_root(Some(root));
    tree
}

/// Mount `root_view` headlessly at `width`×`height`, drive its bootstrap frame,
/// and hand the composited `LayerTree` to `raster` while the binding that owns
/// it is still alive.
fn capture<V: IntoView + 'static>(
    root_view: V,
    width: u32,
    height: u32,
    raster: &impl Fn(&LayerTree) -> Vec<u8>,
) -> Vec<u8> {
    let mut binding = HeadlessBinding::new();

    // The presentation scopes this crate supplies; `flui-testing` owns the
    // mount ordering and the bootstrap frame itself (`mount_root`), so this
    // capture is driven by the SAME sequence `pump_frame` and the live
    // `draw_frame` run — including the layout<->build fixpoint, without which
    // build-during-layout content (a `SliverAppBar`'s delegate child, any
    // persistent-header body) captures as empty space.
    let focused_root = FocusRoot::new(root_view);
    let animated_root = VsyncScope::new(binding.vsync().clone(), focused_root);
    let scoped_root = GestureArenaScope::new(binding.arena().clone(), animated_root);

    let mounted = binding.mount_root(
        &scoped_root,
        MountOwners::fresh(),
        MountOptions::tight(width as f32, height as f32),
    );
    assert!(
        mounted.painted,
        "the bootstrap frame must produce a LayerTree"
    );

    raster(
        binding
            .layer_tree()
            .expect("the bootstrap frame committed a layer tree"),
    )
}
