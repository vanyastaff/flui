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
//! `animated-box` | `colored-box` | `text`.
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
#[path = "text_app.rs"]
mod text_app;
#[allow(dead_code, unused_imports)]
#[path = "widgets_gallery.rs"]
mod widgets_gallery;

use std::sync::Arc;

use flui_binding::HeadlessBinding;
use flui_engine::wgpu::HeadlessRenderer;
use flui_layer::LayerTree;
use flui_rendering::constraints::BoxConstraints;
use flui_rendering::pipeline::PipelineOwner;
use flui_types::Size;
use flui_types::geometry::px;
use flui_view::{BuildOwner, ElementTree, IntoView};
use flui_widgets::VsyncScope;
use parking_lot::RwLock;

fn main() {
    let mut args = std::env::args().skip(1);
    let demo = args.next().unwrap_or_else(|| "material".to_string());
    let width: u32 = args.next().and_then(|s| s.parse().ok()).unwrap_or(900);
    let height: u32 = args.next().and_then(|s| s.parse().ok()).unwrap_or(760);
    let out_path = args.next().unwrap_or_else(|| format!("{demo}.png"));

    // Each arm mounts a different concrete root but returns the same
    // `LayerTree`, so the raster/encode tail is shared below.
    let layer_tree = match demo.as_str() {
        "material" => render_view_to_layers(material_demo::MaterialDemoApp, width, height),
        "cupertino" => render_view_to_layers(cupertino_demo::CupertinoDemoApp, width, height),
        "vertical-slice" | "vslice" => {
            render_view_to_layers(vertical_slice_demo::DemoApp, width, height)
        }
        "gallery" => render_view_to_layers(widgets_gallery::Gallery, width, height),
        "animated-box" => render_view_to_layers(animated_box_app::App, width, height),
        "colored-box" => render_view_to_layers(colored_box_app::App, width, height),
        "text" => render_view_to_layers(text_app::App, width, height),
        other => {
            eprintln!(
                "unknown demo {other:?}; expected: material | cupertino | vertical-slice | \
                 gallery | animated-box | colored-box | text"
            );
            std::process::exit(2);
        }
    };

    let renderer = HeadlessRenderer::new().expect("a GPU device for headless capture");
    let rgba = renderer
        .render_layer_tree(&layer_tree, (width, height))
        .expect("headless render of the demo layer tree");

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

/// Mount `root_view` headlessly at `width`×`height` and drive one frame,
/// returning the composited `LayerTree`.
fn render_view_to_layers<V: IntoView + 'static>(
    root_view: V,
    width: u32,
    height: u32,
) -> LayerTree {
    let binding = HeadlessBinding::new();
    let mut build_owner = BuildOwner::new();
    let mut element_tree = ElementTree::new();
    let pipeline_owner = Arc::new(RwLock::new(PipelineOwner::new()));

    // Wire the async-driver / post-frame / interaction capabilities onto the
    // owner before the mount build pass (matches the acceptance-test bootstrap).
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
            px(width as f32),
            px(height as f32),
        ))));
    }

    // `PipelineOwner::run_frame` runs layout → compositing → paint and returns
    // the composited `LayerTree` (same extraction as `RendererBinding::
    // draw_frame`): take the owner out of the lock by value, run the frame,
    // restore it. Driving the render frame directly on the freshly-built tree
    // keeps its paint dirty — a prior widgets-layer frame would have consumed it.
    let layer_tree = binding.enter_owner_scope(|| {
        let mut guard = pipeline_owner.write();
        let owner = std::mem::take(&mut *guard);
        let (owner, result) = owner.run_frame();
        *guard = owner;
        result.expect("the render frame must succeed")
    });

    layer_tree.expect("the render frame must produce a LayerTree")
}
