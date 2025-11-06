//! Minimal working example - Text rendering
//!
//! This demonstrates the simplest possible Flui app with NEW View API.
//! Shows a single Text widget rendered through the full pipeline.

use flui_app::run_app;
use flui_core::view::{ChangeFlags, View};
use flui_core::{BuildContext, Element, RenderElement, RenderNode};
use flui_rendering::{ParagraphData, RenderParagraph};
use flui_types::Color;

/// Minimal app that displays "Hello, Flui!"
#[derive(Debug, Clone)]
struct MinimalApp;

impl View for MinimalApp {
    type Element = Element;
    type State = ();

    fn build(self, _ctx: &mut BuildContext) -> (Self::Element, Self::State) {
        // Create paragraph data
        let data = ParagraphData::new("Flui Text от системы!")
            .with_font_size(48.0)
            .with_color(Color::rgb(0, 255, 0)); // GREEN text

        // Create RenderParagraph (LeafRender)
        let render_paragraph = RenderParagraph::new(data);

        // Wrap in RenderNode::Leaf
        let render_node = RenderNode::Leaf(Box::new(render_paragraph));

        // Create RenderElement
        let render_element = RenderElement::new(render_node);

        // Return as Element enum
        (Element::Render(render_element), ())
    }

    fn rebuild(
        self,
        _prev: &Self,
        _state: &mut Self::State,
        _element: &mut Self::Element,
    ) -> ChangeFlags {
        ChangeFlags::NONE
    }
}

fn main() -> Result<(), eframe::Error> {
    // Initialize tracing
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::DEBUG)
        .init();

    println!("=== Minimal Flui Demo - Text Rendering ===");
    println!("Starting application with NEW View architecture...");
    println!();

    // Run the app!
    run_app(Box::new(MinimalApp))
}
