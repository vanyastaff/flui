//! Macro Showcase Example - Demonstrates improved FLUI macros
//!
//! This example showcases the enhanced macro system with:
//! - flui_widgets::text! macro for compact text widgets
//! - flui_widgets::sized_box! macro for spacing
//! - flui_widgets::column! macro with multiple syntax options
//! - flui_widgets::row! macro with multiple syntax options
//! - flui_widgets::scaffold! macro with builder-style properties

use flui_app::run_app;
use flui_core::view::{IntoElement, View};
use flui_core::BuildContext;
use flui_types::layout::{CrossAxisAlignment, MainAxisAlignment};
use flui_types::{Color, EdgeInsets};
use flui_widgets::{
    column, row, scaffold, sized_box, text, Button, Card, Center, Divider, Padding,
};

/// Macro showcase application
#[derive(Debug, Clone)]
struct MacroShowcaseApp;

impl View for MacroShowcaseApp {
    fn build(self, _ctx: &BuildContext) -> impl IntoElement {
        // Using improved flui_widgets::scaffold! macro with field initialization
        flui_widgets::scaffold! {
            background_color: Color::rgb(245, 245, 250),
            body: Padding::builder()
                .padding(EdgeInsets::all(24.0))
                .child(
                    Center::builder().child(
                        Card::builder()
                            .elevation(3.0)
                            .child(
                                Padding::builder()
                                    .padding(EdgeInsets::all(24.0))
                                    .child(
                                        // Using flui_widgets::column! macro with children bracket syntax
                                        flui_widgets::column![
                                            // Headline
                                            flui_widgets::text! {
                                                data: "FLUI Macro Showcase",
                                                size: 28.0,
                                                color: Color::rgb(33, 33, 33)
                                            },

                                            flui_widgets::sized_box! { height: 8.0 },

                                            // Subtitle
                                            flui_widgets::text! {
                                                data: "Improved macro system demonstration",
                                                size: 14.0,
                                                color: Color::rgb(117, 117, 117)
                                            },

                                            flui_widgets::sized_box! { height: 20.0 },

                                            Divider::builder()
                                                .color(Color::rgb(224, 224, 224))
                                                .build(),

                                            flui_widgets::sized_box! { height: 20.0 },

                                            // Section 1: flui_widgets::column! with properties
                                            flui_widgets::text! {
                                                data: "1. Column with properties:",
                                                size: 16.0,
                                                color: Color::rgb(33, 33, 33)
                                            },

                                            flui_widgets::sized_box! { height: 12.0 },

                                            // Using flui_widgets::column! with properties and children
                                            flui_widgets::column! {
                                                cross_axis_alignment: CrossAxisAlignment::Start;
                                                [
                                                    flui_widgets::text! {
                                                        data: "• Start-aligned item 1",
                                                        size: 14.0,
                                                        color: Color::rgb(66, 66, 66)
                                                    },
                                                    flui_widgets::sized_box! { height: 4.0 },
                                                    flui_widgets::text! {
                                                        data: "• Start-aligned item 2",
                                                        size: 14.0,
                                                        color: Color::rgb(66, 66, 66)
                                                    }
                                                ]
                                            },

                                            flui_widgets::sized_box! { height: 20.0 },

                                            // Section 2: flui_widgets::row! with properties
                                            flui_widgets::text! {
                                                data: "2. Row with space-evenly:",
                                                size: 16.0,
                                                color: Color::rgb(33, 33, 33)
                                            },

                                            flui_widgets::sized_box! { height: 12.0 },

                                            // Using flui_widgets::row! with properties and children
                                            flui_widgets::row! {
                                                main_axis_alignment: MainAxisAlignment::SpaceEvenly;
                                                [
                                                    Button::builder("One")
                                                        .color(Color::rgb(66, 133, 244))
                                                        .build(),
                                                    Button::builder("Two")
                                                        .color(Color::rgb(52, 168, 83))
                                                        .build(),
                                                    Button::builder("Three")
                                                        .color(Color::rgb(251, 188, 4))
                                                        .build()
                                                ]
                                            },

                                            flui_widgets::sized_box! { height: 20.0 },

                                            // Section 3: Simple flui_widgets::row!
                                            flui_widgets::text! {
                                                data: "3. Simple row syntax:",
                                                size: 16.0,
                                                color: Color::rgb(33, 33, 33)
                                            },

                                            flui_widgets::sized_box! { height: 12.0 },

                                            // Using simple flui_widgets::row! bracket syntax
                                            flui_widgets::row![
                                                flui_widgets::text! {
                                                    data: "Item A",
                                                    size: 14.0,
                                                    color: Color::rgb(66, 66, 66)
                                                },
                                                flui_widgets::sized_box! { width: 16.0 },
                                                flui_widgets::text! {
                                                    data: "Item B",
                                                    size: 14.0,
                                                    color: Color::rgb(66, 66, 66)
                                                },
                                                flui_widgets::sized_box! { width: 16.0 },
                                                flui_widgets::text! {
                                                    data: "Item C",
                                                    size: 14.0,
                                                    color: Color::rgb(66, 66, 66)
                                                }
                                            ],

                                            flui_widgets::sized_box! { height: 24.0 },

                                            // Footer
                                            Divider::builder()
                                                .color(Color::rgb(224, 224, 224))
                                                .build(),

                                            flui_widgets::sized_box! { height: 16.0 },

                                            Center::builder()
                                                .child(flui_widgets::text! {
                                                    data: "All macros support multiple syntax styles!",
                                                    size: 12.0,
                                                    color: Color::rgb(117, 117, 117)
                                                })
                                                .build()
                                        ]
                                    )
                                    .build()
                            )
                            .build()
                    )
                    .build()
                )
                .build()
        }
    }
}

fn main() -> Result<(), eframe::Error> {
    // Initialize tracing
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .init();

    println!("=== FLUI Macro Showcase ==");
    println!("Demonstrates improved macro system with:");
    println!("  • flui_widgets::text! - Compact text widget creation");
    println!("  • flui_widgets::sized_box! - Easy spacing");
    println!("  • flui_widgets::column! - Multiple syntax styles (bracket, brace, combined)");
    println!("  • flui_widgets::row! - Multiple syntax styles (bracket, brace, combined)");
    println!("  • flui_widgets::scaffold! - Builder-style property initialization");
    println!();

    run_app(Box::new(MacroShowcaseApp))
}
