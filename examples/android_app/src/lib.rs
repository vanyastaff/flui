//! Widget-based hot-reloadable plugin for FLUI Android demo.
//!
//! Uses `app_plugin!` to run a full widget pipeline (View → Element → Render → Scene)
//! inside the plugin. The host gets back an opaque `Scene` — same as `scene_plugin!`,
//! but the user writes normal widget code instead of raw canvas commands.
//!
//! # Hot-Reload
//!
//! Edit this file → `cargo ndk build` → push `.so` → app reloads automatically.
//! Each reload re-mounts the widget tree from scratch ("hot restart" semantics).

use flui_hot_reload::app_plugin;
use flui_objects::RenderColoredBox;
use flui_rendering::protocol::BoxProtocol;
use flui_types::Size;
use flui_types::geometry::px;
use flui_view::impl_render_view;
use flui_view::prelude::*;

// ---------------------------------------------------------------------------
// ColoredBoxView — a simple View wrapper around RenderColoredBox
// ---------------------------------------------------------------------------

/// A leaf View that paints a colored rectangle.
///
/// Wraps [`RenderColoredBox`] from the rendering layer.
#[derive(Clone)]
struct ColoredBoxView {
    color: [f32; 4],
    width: f32,
    height: f32,
}

impl ColoredBoxView {
    fn new(color: [f32; 4], width: f32, height: f32) -> Self {
        Self {
            color,
            width,
            height,
        }
    }
}

impl RenderView for ColoredBoxView {
    type Protocol = BoxProtocol;
    type RenderObject = RenderColoredBox;

    fn create_render_object(&self) -> Self::RenderObject {
        RenderColoredBox::new(self.color, Size::new(px(self.width), px(self.height)))
    }

    fn update_render_object(&self, _render_object: &mut Self::RenderObject) {
        // RenderColoredBox is immutable after creation — on update, the element
        // will recreate it (hot restart remounts the whole tree anyway).
    }
}

impl_render_view!(ColoredBoxView);

// ---------------------------------------------------------------------------
// MyApp — the root widget
// ---------------------------------------------------------------------------

/// The root application widget.
///
/// Change the color values here and rebuild to see hot-reload in action.
#[derive(Clone, StatelessView)]
struct MyApp;

impl StatelessView for MyApp {
    fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
        // Purple background — edit this color and hot-reload!
        ColoredBoxView::new(
            [0.5, 0.0, 0.5, 1.0], // RGBA: purple
            4096.0,               // large enough to fill any screen
            4096.0,
        )
    }
}

// ---------------------------------------------------------------------------
// FFI entry point — generates flui_app_build / flui_app_version / flui_app_drop
// ---------------------------------------------------------------------------

app_plugin!(MyApp);
