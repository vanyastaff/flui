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
#[path = "text_app.rs"]
mod text_app;
#[allow(dead_code, unused_imports)]
#[path = "widgets_gallery.rs"]
mod widgets_gallery;

use flui_engine::wgpu::HeadlessRenderer;
use flui_layer::{Layer, LayerTree, PerformanceOverlayLayer};
use flui_rendering::constraints::BoxConstraints;
use flui_rendering::pipeline::{PipelineCell, PipelineOwner};
use flui_testing::HeadlessBinding;
use flui_types::Size;
use flui_types::geometry::px;
use flui_view::{BuildOwner, ElementTree, IntoView};
use flui_widgets::{FocusRoot, GestureArenaScope, VsyncScope};

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
        "animated-box" => render_view_to_layers(animated_box_app::App::new(), width, height),
        "colored-box" => render_view_to_layers(colored_box_app::App, width, height),
        "text" => render_view_to_layers(text_app::App, width, height),
        "telemetry-overlay" => telemetry_overlay_layers(),
        // The collapsing-sliver scene at three scroll depths — a visual
        // check on the SliverAppBar / FlexibleSpaceBar / pinned-header
        // pipeline (expanded, mid-collapse with the background fading and
        // parallaxing, and fully collapsed to the pinned toolbar).
        "sliver" => render_view_to_layers(sliver_demo(0.0), width, height),
        "sliver-mid" => render_view_to_layers(sliver_demo(90.0), width, height),
        "sliver-collapsed" => render_view_to_layers(sliver_demo(500.0), width, height),
        other => {
            eprintln!(
                "unknown demo {other:?}; expected: material | cupertino | vertical-slice | \
                 gallery | animated-box | colored-box | text | telemetry-overlay | \
                 sliver | sliver-mid | sliver-collapsed"
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

/// A Material collapsing-header scene: pinned `SliverAppBar` with a
/// `FlexibleSpaceBar` (title + gradient-ish colored background) over a list
/// of labeled rows, mounted pre-scrolled to `offset`.
fn sliver_demo(offset: f32) -> impl IntoView {
    use flui::material::{FlexibleSpaceBar, SliverAppBar, Theme, ThemeData};
    use flui::widgets::{
        ColoredBox, CustomScrollView, MediaQuery, MediaQueryData, Padding, SizedBox,
        SliverToBoxAdapter, Text,
    };
    use flui_types::{Color, EdgeInsets};
    use flui_view::view::ViewExt;

    // The title lives in the flexible space only (the usual collapsing-bar
    // shape): it rides the collapse, scaling from 1.5x at the bottom edge
    // to toolbar size.
    let bar = SliverAppBar::new()
        .expanded_height(220.0)
        .pinned(true)
        .flexible_space(
            FlexibleSpaceBar::new()
                .title(Text::new("FLUI Slivers"))
                .background(ColoredBox::new(Color::rgb(21, 101, 192))),
        );

    let mut slivers: Vec<flui_view::BoxedView> = vec![bar.into_view().boxed()];
    for i in 0..14 {
        let shade = if i % 2 == 0 { 245 } else { 232 };
        slivers.push(
            SliverToBoxAdapter::new()
                .child(ColoredBox::new(Color::rgb(shade, shade, shade)).child(
                    SizedBox::new(0.0, 56.0).child(Padding::new(EdgeInsets::all(px(16.0))).child(
                        Text::new(format!("Row {i} — scrolled under a pinned SliverAppBar")),
                    )),
                ))
                .into_view()
                .boxed(),
        );
    }

    Theme::new(
        ThemeData::light(),
        MediaQuery::new(
            MediaQueryData::default(),
            CustomScrollView::new(slivers).offset(offset),
        ),
    )
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
    let pipeline_owner = PipelineCell::new(PipelineOwner::new());

    // Wire the async-driver / post-frame / interaction capabilities onto the
    // owner before the mount build pass (matches the acceptance-test bootstrap).
    binding.install_build_capabilities(&mut build_owner);

    let focused_root = FocusRoot::new(root_view);
    let animated_root = VsyncScope::new(binding.vsync().clone(), focused_root);
    let scoped_root = GestureArenaScope::new(binding.arena().clone(), animated_root);

    binding.enter_owner_scope(|| {
        let root_element = element_tree.mount_root_with_pipeline_owner(
            &scoped_root,
            Some(pipeline_owner.clone()),
            &mut build_owner.element_owner_mut(),
        );
        build_owner.schedule_build_for(root_element, 0, flui_view::RebuildReason::InitialMount);
        build_owner.build_scope(&mut element_tree);
    });

    let root_render_id = pipeline_owner.with(|owner| {
        let render_tree = owner.render_tree();
        render_tree
            .iter()
            .map(|(id, _)| id)
            .find(|id| render_tree.parent(*id).is_none())
            .expect("the mounted demo tree must have a render root")
    });

    pipeline_owner.with_mut(|owner| {
        owner.set_root_id(Some(root_render_id));
        owner.set_root_constraints(Some(BoxConstraints::tight(Size::new(
            px(width as f32),
            px(height as f32),
        ))));
    });

    // The layout↔build fixpoint frame — the SAME helper `HeadlessBinding`'s
    // pump and the live `draw_frame` use. A bare `PipelineOwner::run_frame`
    // never services build-during-layout content (a `SliverAppBar`'s
    // delegate child, any persistent-header body), so a hand-rolled frame
    // here captured collapsing app bars as empty space.
    let layer_tree = binding.enter_owner_scope(|| {
        build_owner
            .run_frame_with_layout_builders(&mut element_tree, &pipeline_owner)
            .expect("the render frame must succeed")
    });

    layer_tree.expect("the render frame must produce a LayerTree")
}
