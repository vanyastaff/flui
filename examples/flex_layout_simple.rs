//! Simple Flex layout example - testing Expanded widgets

use flui_app::*;
use flui_widgets::prelude::*;
use flui_widgets::{Expanded, DynWidget};

#[derive(Debug, Clone)]
struct FlexLayoutExample;

impl StatelessWidget for FlexLayoutExample {
    fn build(&self, _context: &BuildContext) -> Box<dyn DynWidget> {
        // Create a Row with three equal Expanded containers
        Box::new(Row::builder()
            .children(vec![
                Box::new(
                    Expanded::new(
                        Container::builder()
                            .color(Color::rgb(255, 0, 0))
                            .height(100.0)
                            .build()
                    )
                ),
                Box::new(
                    Expanded::new(
                        Container::builder()
                            .color(Color::rgb(0, 255, 0))
                            .height(100.0)
                            .build()
                    )
                ),
                Box::new(
                    Expanded::new(
                        Container::builder()
                            .color(Color::rgb(0, 0, 255))
                            .height(100.0)
                            .build()
                    )
                ),
            ])
            .build())
    }
}

fn main() {
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .init();

    tracing::info!("Testing Flex layout with 3 equal Expanded containers");

    run_app(Box::new(FlexLayoutExample)).unwrap()
}
