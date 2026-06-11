//! Text — proves the text pipeline end-to-end through the REAL framework.
//!
//! The sibling of `colored_box_app`, but for the last content leaf:
//!
//! ```text
//! View (TextLabel) → Element tree → RenderParagraph
//!   → layout (TextPainter / cosmic-text) → paint (DrawTextSpan)
//!   → glyphon rich-text rasterization → wgpu
//! ```
//!
//! Renders "Hello, FLUI!" with the word `FLUI` in bold red — so it also
//! proves the per-span rich-text seam (Wave 2b): a styled child span keeps
//! its own color and weight instead of collapsing to plain black SansSerif.
//!
//! Run with: cargo run -p flui --example text_app

use flui_app::run_app;
use flui_rendering::objects::RenderParagraph;
use flui_types::{
    Color,
    typography::{FontWeight, TextDirection, TextSpan, TextStyle},
};
use flui_view::{BuildContext, ElementBase, IntoView, RenderView, StatelessView, View, ViewExt};

/// Builds the styled "Hello, FLUI!" span: a 48px dark base with one bold-red
/// child word, so the rich-text path (per-span color + weight) is exercised.
fn greeting() -> TextSpan {
    let base = TextStyle {
        font_size: Some(48.0),
        color: Some(Color::rgb(30, 30, 30)),
        ..Default::default()
    };
    let accent = TextStyle {
        color: Some(Color::rgb(200, 30, 30)),
        font_weight: Some(FontWeight::W700),
        ..Default::default()
    };
    TextSpan::new("")
        .with_style(base)
        .with_child(TextSpan::new("Hello, "))
        .with_child(TextSpan::styled("FLUI", accent))
        .with_child(TextSpan::new("!"))
}

/// Leaf render view producing a [`RenderParagraph`] for the greeting.
#[derive(Clone)]
struct TextLabel;

impl RenderView for TextLabel {
    type Protocol = flui_rendering::protocol::BoxProtocol;
    type RenderObject = RenderParagraph;

    fn create_render_object(&self) -> Self::RenderObject {
        RenderParagraph::new(greeting(), TextDirection::Ltr)
    }

    fn update_render_object(&self, render_object: &mut Self::RenderObject) {
        render_object.set_text(greeting());
    }
}

flui_view::impl_render_view!(TextLabel);

/// Stateless root that builds the label.
#[derive(Clone)]
struct App;

impl StatelessView for App {
    fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
        TextLabel.boxed()
    }
}

impl View for App {
    fn create_element(&self) -> Box<dyn ElementBase> {
        Box::new(flui_view::StatelessElement::new(
            self,
            flui_view::element::StatelessBehavior,
        ))
    }
}

fn main() {
    run_app(App);
}
