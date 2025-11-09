//! Simple layout test - debug vertical layout

use flui_app::run_app;
use flui_core::view::{IntoElement, View};
use flui_core::BuildContext;
use flui_types::{Color, EdgeInsets};
use flui_widgets::{Column, Container, Text};

#[derive(Debug, Clone)]
struct SimpleLayoutApp;

impl View for SimpleLayoutApp {
    fn build(self, _ctx: &BuildContext) -> impl IntoElement {
        let mut main_column = Column::builder().build();

        // Top box (like AppBar)
        let mut top = Container::builder()
            .height(64.0)
            .color(Color::rgb(255, 0, 0)) // RED
            .padding(EdgeInsets::all(20.0))
            .build();
        top.child = Some(Box::new(
            Text::builder()
                .data("TOP (RED)")
                .size(20.0)
                .color(Color::WHITE)
                .build(),
        ));
        main_column.children.push(Box::new(top));

        // Middle box (like Body)
        let mut middle = Container::builder()
            .color(Color::rgb(0, 255, 0)) // GREEN
            .padding(EdgeInsets::all(20.0))
            .build();
        middle.child = Some(Box::new(
            Text::builder()
                .data("MIDDLE (GREEN) - Should fill remaining space")
                .size(16.0)
                .color(Color::BLACK)
                .build(),
        ));
        main_column.children.push(Box::new(middle));

        // Bottom box (like BottomNav)
        let mut bottom = Container::builder()
            .height(56.0)
            .color(Color::rgb(0, 0, 255)) // BLUE
            .padding(EdgeInsets::all(16.0))
            .build();
        bottom.child = Some(Box::new(
            Text::builder()
                .data("BOTTOM (BLUE)")
                .size(14.0)
                .color(Color::WHITE)
                .build(),
        ));
        main_column.children.push(Box::new(bottom));

        main_column
    }
}

fn main() -> Result<(), eframe::Error> {
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .init();

    println!("=== Simple Layout Test ===");
    println!("Expect: RED top bar, GREEN middle (fills), BLUE bottom bar");
    println!();

    run_app(Box::new(SimpleLayoutApp))
}
