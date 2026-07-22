//! Colored Box — the first FLUI application through the REAL pipeline.
//!
//! Unlike `direct_render` (which bypasses the framework and hand-builds a
//! Scene), this example goes the whole way:
//!
//! ```text
//! View (ColoredSquare) → Element tree → RenderColoredBox
//!   → layout (RenderState offsets) → paint (fragment recording)
//!   → LayerTree (merged PictureLayer) → Scene → wgpu
//! ```
//!
//! A red 200×200 square on the window background proves the pipeline's
//! exit gate: build, layout, the sans-IO fragment paint walk, scene
//! composition, and GPU submission all fire for real.
//!
//! Run with: cargo run --example colored_box_app

use flui_app::run_app;
use flui_objects::RenderColoredBox;
use flui_view::{BuildContext, IntoView, RenderView, StatelessView, View, ViewExt};

/// Leaf render view producing a red 200×200 [`RenderColoredBox`].
#[derive(Clone)]
struct ColoredSquare;

impl RenderView for ColoredSquare {
    type Protocol = flui_rendering::protocol::BoxProtocol;
    type RenderObject = RenderColoredBox;

    fn create_render_object(
        &self,
        _ctx: &flui_view::RenderObjectContext<'_>,
    ) -> Self::RenderObject {
        RenderColoredBox::red(200.0, 200.0)
    }

    fn update_render_object(
        &self,
        _ctx: &flui_view::RenderObjectContext<'_>,
        render_object: &mut Self::RenderObject,
    ) {
        *render_object = RenderColoredBox::red(200.0, 200.0);
    }
}

flui_view::impl_render_view!(ColoredSquare);

/// Stateless root that builds the square.
#[derive(Clone)]
struct App;

impl StatelessView for App {
    fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
        ColoredSquare.boxed()
    }
}

impl View for App {
    fn create_element(&self) -> flui_view::element::ElementKind {
        flui_view::element::ElementKind::stateless(self)
    }
}

fn main() {
    run_app(App);
}
