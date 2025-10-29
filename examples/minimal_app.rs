//! Minimal High-Level App Example
//!
//! Demonstrates flui_app without flui_widgets dependency.
//! Shows how to create a custom widget that paints directly.

use flui_app::{BuildContext, StatelessWidget, run_app};
use flui_core::{BoxedWidget, DynWidget, Widget};
use flui_engine::{Layer, Paint, Painter, PictureLayer};
use flui_types::{Color, Rect, Size};

/// A minimal custom widget that paints a colored rectangle
#[derive(Debug, Clone)]
struct ColoredBox {
    color: Color,
    size: Size,
}

impl ColoredBox {
    fn new(color: Color, width: f32, height: f32) -> Self {
        Self {
            color,
            size: Size::new(width, height),
        }
    }
}

impl Widget for ColoredBox {
    fn paint(&self, painter: &mut dyn Painter) {
        let mut paint = Paint::default();
        paint.color = self.color;

        let rect = Rect::from_xywh(0.0, 0.0, self.size.width, self.size.height);
        painter.rect(rect, &paint);
    }
}

impl DynWidget for ColoredBox {
    fn as_widget(&self) -> &dyn Widget {
        self
    }
}

/// Root application widget
#[derive(Debug, Clone)]
struct MinimalApp;

impl StatelessWidget for MinimalApp {
    fn build(&self, _ctx: &BuildContext) -> BoxedWidget {
        Box::new(ColoredBox::new(
            Color::rgb(66, 165, 245), // Blue
            400.0,
            300.0,
        ))
    }
}

impl Widget for MinimalApp {}
impl DynWidget for MinimalApp {
    fn as_widget(&self) -> &dyn Widget {
        self
    }
}

fn main() -> Result<(), eframe::Error> {
    println!("=== Minimal Flui App ===");
    println!("High-level API without flui_widgets");
    println!();
    println!("Architecture:");
    println!("  MinimalApp (StatelessWidget)");
    println!("    → build() returns ColoredBox");
    println!("    → ColoredBox.paint() draws blue rectangle");
    println!();

    run_app(Box::new(MinimalApp))
}
