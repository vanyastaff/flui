//! Flex layout example - testing Flexible and Expanded widgets
//!
//! Demonstrates:
//! - Flexible widgets with different flex factors
//! - Expanded widgets (FlexFit::Tight)
//! - Mixed fixed and flexible children
//! - Row and Column layouts

use flui_app::*;
use flui_widgets::prelude::*;
use flui_widgets::{Expanded, Flexible, DynWidget};

#[derive(Debug, Clone)]
struct FlexLayoutExample;

impl StatelessWidget for FlexLayoutExample {
    fn build(&self, _context: &BuildContext) -> Box<dyn DynWidget> {
        // Create a Column with multiple flex layout examples
        Box::new(Column::builder()
            .main_axis_alignment(MainAxisAlignment::Start)
            .cross_axis_alignment(CrossAxisAlignment::Stretch)
            .children(vec![
                // Example 1: Equal width columns with Expanded
                Box::new(
                    Container::builder()
                        .padding(EdgeInsets::all(16.0))
                        .child(
                            Column::builder()
                                .cross_axis_alignment(CrossAxisAlignment::Start)
                                .children(vec![
                                    Box::new(Text::new("Example 1: Three Equal Columns (Expanded)")),
                                    Box::new(SizedBox::builder().height(8.0).build()),
                                    Box::new(
                                        Row::builder()
                                            .children(vec![
                                                Box::new(
                                                    Expanded::new(
                                                        Container::builder()
                                                            .color(Color::rgb(255, 0, 0))
                                                            .height(60.0)
                                                            .build()
                                                    )
                                                ),
                                                Box::new(
                                                    Expanded::new(
                                                        Container::builder()
                                                            .color(Color::rgb(0, 255, 0))
                                                            .height(60.0)
                                                            .build()
                                                    )
                                                ),
                                                Box::new(
                                                    Expanded::new(
                                                        Container::builder()
                                                            .color(Color::rgb(0, 0, 255))
                                                            .height(60.0)
                                                            .build()
                                                    )
                                                ),
                                            ])
                                            .build()
                                    ),
                                ])
                                .build()
                        )
                        .build()
                ),

                // Example 2: Proportional width columns (1:2:1)
                Box::new(
                    Container::builder()
                        .padding(EdgeInsets::all(16.0))
                        .child(
                            Column::builder()
                                .cross_axis_alignment(CrossAxisAlignment::Start)
                                .children(vec![
                                    Box::new(Text::new("Example 2: Proportional Columns (1:2:1)")),
                                    Box::new(SizedBox::builder().height(8.0).build()),
                                    Box::new(
                                        Row::builder()
                                            .children(vec![
                                                Box::new(
                                                    Expanded::with_flex(1,
                                                        Container::builder()
                                                            .color(Color::rgb(255, 128, 0))
                                                            .height(60.0)
                                                            .build()
                                                    )
                                                ),
                                                Box::new(SizedBox::builder().width(8.0).build()),
                                                Box::new(
                                                    Expanded::with_flex(2,
                                                        Container::builder()
                                                            .color(Color::rgb(128, 0, 255))
                                                            .height(60.0)
                                                            .build()
                                                    )
                                                ),
                                                Box::new(SizedBox::builder().width(8.0).build()),
                                                Box::new(
                                                    Expanded::with_flex(1,
                                                        Container::builder()
                                                            .color(Color::rgb(0, 255, 255))
                                                            .height(60.0)
                                                            .build()
                                                    )
                                                ),
                                            ])
                                            .build()
                                    ),
                                ])
                                .build()
                        )
                        .build()
                ),

                // Example 3: Mixed fixed and flexible
                Box::new(
                    Container::builder()
                        .padding(EdgeInsets::all(16.0))
                        .child(
                            Column::builder()
                                .cross_axis_alignment(CrossAxisAlignment::Start)
                                .children(vec![
                                    Box::new(Text::new("Example 3: Fixed Sidebars + Flexible Content")),
                                    Box::new(SizedBox::builder().height(8.0).build()),
                                    Box::new(
                                        Row::builder()
                                            .children(vec![
                                                // Fixed left sidebar
                                                Box::new(
                                                    Container::builder()
                                                        .color(Color::rgb(100, 100, 100))
                                                        .width(60.0)
                                                        .height(60.0)
                                                        .build()
                                                ),
                                                Box::new(SizedBox::builder().width(8.0).build()),
                                                // Flexible content
                                                Box::new(
                                                    Expanded::new(
                                                        Container::builder()
                                                            .color(Color::rgb(200, 200, 200))
                                                            .height(60.0)
                                                            .build()
                                                    )
                                                ),
                                                Box::new(SizedBox::builder().width(8.0).build()),
                                                // Fixed right sidebar
                                                Box::new(
                                                    Container::builder()
                                                        .color(Color::rgb(100, 100, 100))
                                                        .width(80.0)
                                                        .height(60.0)
                                                        .build()
                                                ),
                                            ])
                                            .build()
                                    ),
                                ])
                                .build()
                        )
                        .build()
                ),

                // Example 4: Flexible (loose fit)
                Box::new(
                    Container::builder()
                        .padding(EdgeInsets::all(16.0))
                        .child(
                            Column::builder()
                                .cross_axis_alignment(CrossAxisAlignment::Start)
                                .children(vec![
                                    Box::new(Text::new("Example 4: Flexible (can be smaller)")),
                                    Box::new(SizedBox::builder().height(8.0).build()),
                                    Box::new(
                                        Row::builder()
                                            .children(vec![
                                                Box::new(
                                                    Flexible::new(1,
                                                        Container::builder()
                                                            .color(Color::rgb(255, 200, 200))
                                                            .width(50.0)
                                                            .height(60.0)
                                                            .build()
                                                    )
                                                ),
                                                Box::new(SizedBox::builder().width(8.0).build()),
                                                Box::new(
                                                    Flexible::new(1,
                                                        Container::builder()
                                                            .color(Color::rgb(200, 255, 200))
                                                            .width(100.0)
                                                            .height(60.0)
                                                            .build()
                                                    )
                                                ),
                                                Box::new(SizedBox::builder().width(8.0).build()),
                                                Box::new(
                                                    Flexible::new(1,
                                                        Container::builder()
                                                            .color(Color::rgb(200, 200, 255))
                                                            .width(75.0)
                                                            .height(60.0)
                                                            .build()
                                                    )
                                                ),
                                            ])
                                            .build()
                                    ),
                                ])
                                .build()
                        )
                        .build()
                ),
            ])
            .build())
    }
}

fn main() {
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::DEBUG)
        .init();

    tracing::info!("========================================");
    tracing::info!("Starting Flex Layout Example");
    tracing::info!("========================================");
    tracing::info!("");
    tracing::info!("This example demonstrates:");
    tracing::info!("  1. Three equal columns (Expanded)");
    tracing::info!("  2. Proportional columns 1:2:1");
    tracing::info!("  3. Fixed sidebars + flexible content");
    tracing::info!("  4. Flexible (loose fit - can be smaller)");
    tracing::info!("========================================");

    run_app(Box::new(FlexLayoutExample)).unwrap()
}
