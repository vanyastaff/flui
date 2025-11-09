//! Style Comparison Example - Three ways to write the same UI
//!
//! This example demonstrates three different coding styles in FLUI:
//! 1. Macro style - maximum declarative, minimal boilerplate
//! 2. Builder style - traditional Rust, explicit and IDE-friendly
//! 3. Hybrid style - best of both worlds (recommended)
//!
//! All three produce identical UI, choose based on your preference!

use flui_app::run_app;
use flui_core::view::{IntoElement, View};
use flui_core::BuildContext;

// =============================================================================
// MACRO STYLE - Compact and declarative
// =============================================================================

mod macro_style {
    use super::*;
    use flui_widgets::style::macros::prelude::*;

    #[derive(Debug, Clone)]
    pub struct MacroStyleDemo;

    impl View for MacroStyleDemo {
        fn build(self, _ctx: &BuildContext) -> impl IntoElement {
            flui_widgets::scaffold! {
                background_color: Color::rgb(245, 245, 250),
                body: Center::builder()
                    .child(
                        flui_widgets::column![
                            text! {
                                data: "Macro Style",
                                size: 32.0,
                                color: Color::rgb(33, 33, 33)
                            },

                            sized_box! { height: 8.0 },

                            text! {
                                data: "Maximum declarative syntax",
                                size: 14.0,
                                color: Color::rgb(117, 117, 117)
                            },

                            sized_box! { height: 24.0 },

                            flui_widgets::row! {
                                main_axis_alignment: MainAxisAlignment::Center;
                                [
                                    text! {
                                        data: "Compact",
                                        size: 16.0,
                                        color: Color::rgb(33, 150, 243)
                                    },
                                    sized_box! { width: 16.0 },
                                    text! {
                                        data: "•",
                                        size: 16.0,
                                        color: Color::rgb(117, 117, 117)
                                    },
                                    sized_box! { width: 16.0 },
                                    text! {
                                        data: "Flutter-like",
                                        size: 16.0,
                                        color: Color::rgb(52, 168, 83)
                                    }
                                ]
                            }
                        ]
                    )
                    .build()
            }
        }
    }
}

// =============================================================================
// BUILDER STYLE - Traditional and explicit
// =============================================================================

mod builder_style {
    use super::*;
    use flui_widgets::style::builder::prelude::*;

    #[derive(Debug, Clone)]
    pub struct BuilderStyleDemo;

    impl View for BuilderStyleDemo {
        fn build(self, _ctx: &BuildContext) -> impl IntoElement {
            Scaffold::builder()
                .background_color(Color::rgb(245, 245, 250))
                .body(
                    Center::builder()
                        .child(
                            Column::builder()
                                .cross_axis_alignment(CrossAxisAlignment::Center)
                                .child(
                                    Text::builder()
                                        .data("Builder Style")
                                        .size(32.0)
                                        .color(Color::rgb(33, 33, 33))
                                        .build(),
                                )
                                .child(SizedBox::builder().height(8.0).build())
                                .child(
                                    Text::builder()
                                        .data("Traditional Rust patterns")
                                        .size(14.0)
                                        .color(Color::rgb(117, 117, 117))
                                        .build(),
                                )
                                .child(SizedBox::builder().height(24.0).build())
                                .child(
                                    Row::builder()
                                        .main_axis_alignment(MainAxisAlignment::Center)
                                        .child(
                                            Text::builder()
                                                .data("Explicit")
                                                .size(16.0)
                                                .color(Color::rgb(33, 150, 243))
                                                .build(),
                                        )
                                        .child(SizedBox::builder().width(16.0).build())
                                        .child(
                                            Text::builder()
                                                .data("•")
                                                .size(16.0)
                                                .color(Color::rgb(117, 117, 117))
                                                .build(),
                                        )
                                        .child(SizedBox::builder().width(16.0).build())
                                        .child(
                                            Text::builder()
                                                .data("IDE-friendly")
                                                .size(16.0)
                                                .color(Color::rgb(52, 168, 83))
                                                .build(),
                                        )
                                        .build(),
                                )
                                .build(),
                        )
                        .build(),
                )
                .build()
        }
    }
}

// =============================================================================
// HYBRID STYLE - Balanced approach (recommended)
// =============================================================================

mod hybrid_style {
    use super::*;
    use flui_widgets::style::hybrid::prelude::*;

    #[derive(Debug, Clone)]
    pub struct HybridStyleDemo;

    impl View for HybridStyleDemo {
        fn build(self, _ctx: &BuildContext) -> impl IntoElement {
            Scaffold::builder()
                .background_color(Color::rgb(245, 245, 250))
                .body(
                    Center::builder()
                        .child(flui_widgets::column![
                            text! {
                                data: "Hybrid Style",
                                size: 32.0,
                                color: Color::rgb(33, 33, 33)
                            },
                            sized_box! { height: 8.0 },
                            text! {
                                data: "Best of both worlds (recommended)",
                                size: 14.0,
                                color: Color::rgb(117, 117, 117)
                            },
                            sized_box! { height: 24.0 },
                            Row::builder()
                                .main_axis_alignment(MainAxisAlignment::Center)
                                .child(text! {
                                    data: "Pragmatic",
                                    size: 16.0,
                                    color: Color::rgb(33, 150, 243)
                                })
                                .child(sized_box! { width: 16.0 })
                                .child(text! {
                                    data: "•",
                                    size: 16.0,
                                    color: Color::rgb(117, 117, 117)
                                })
                                .child(sized_box! { width: 16.0 })
                                .child(text! {
                                    data: "Flexible",
                                    size: 16.0,
                                    color: Color::rgb(52, 168, 83)
                                })
                                .build()
                        ])
                        .build(),
                )
                .build()
        }
    }
}

// =============================================================================
// Main app - shows all three styles side by side
// =============================================================================

use flui_types::{Color, EdgeInsets};
use flui_widgets::{Card, Center, Column, Padding, Row, Scaffold, Text};

#[derive(Debug, Clone)]
struct StyleComparisonApp;

impl View for StyleComparisonApp {
    fn build(self, _ctx: &BuildContext) -> impl IntoElement {
        use flui_types::layout::{CrossAxisAlignment, MainAxisAlignment};

        Scaffold::builder()
            .background_color(Color::rgb(245, 245, 250))
            .body(
                Padding::builder()
                    .padding(EdgeInsets::all(24.0))
                    .child(
                        Column::builder()
                            .cross_axis_alignment(CrossAxisAlignment::Center)
                            // Title
                            .child(
                                Text::builder()
                                    .data("FLUI Style Comparison")
                                    .size(28.0)
                                    .color(Color::rgb(33, 33, 33))
                                    .build(),
                            )
                            .child(
                                Text::builder()
                                    .data("Three ways to write the same UI")
                                    .size(14.0)
                                    .color(Color::rgb(117, 117, 117))
                                    .build(),
                            )
                            // Three columns with demos
                            .child(
                                Padding::builder()
                                    .padding(EdgeInsets::symmetric(0.0, 32.0))
                                    .child(
                                        Row::builder()
                                            .main_axis_alignment(MainAxisAlignment::SpaceEvenly)
                                            .cross_axis_alignment(CrossAxisAlignment::Start)
                                            // Macro style
                                            .child(
                                                Card::builder()
                                                    .elevation(4.0)
                                                    .child(
                                                        Padding::builder()
                                                            .padding(EdgeInsets::all(24.0))
                                                            .child(macro_style::MacroStyleDemo)
                                                            .build(),
                                                    )
                                                    .build(),
                                            )
                                            // Builder style
                                            .child(
                                                Card::builder()
                                                    .elevation(4.0)
                                                    .child(
                                                        Padding::builder()
                                                            .padding(EdgeInsets::all(24.0))
                                                            .child(builder_style::BuilderStyleDemo)
                                                            .build(),
                                                    )
                                                    .build(),
                                            )
                                            // Hybrid style
                                            .child(
                                                Card::builder()
                                                    .elevation(4.0)
                                                    .child(
                                                        Padding::builder()
                                                            .padding(EdgeInsets::all(24.0))
                                                            .child(hybrid_style::HybridStyleDemo)
                                                            .build(),
                                                    )
                                                    .build(),
                                            )
                                            .build(),
                                    )
                                    .build(),
                            )
                            .build(),
                    )
                    .build(),
            )
            .build()
    }
}

fn main() -> Result<(), eframe::Error> {
    // Initialize tracing
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .init();

    println!("=== FLUI Style Comparison ===");
    println!("Demonstrates three coding styles:");
    println!("  1. Macro Style - compact and declarative");
    println!("  2. Builder Style - traditional and explicit");
    println!("  3. Hybrid Style - best of both (recommended)");
    println!();
    println!("Choose your preferred style with:");
    println!("  use flui_widgets::style::macros::prelude::*;  // Macro");
    println!("  use flui_widgets::style::builder::prelude::*; // Builder");
    println!("  use flui_widgets::style::hybrid::prelude::*;  // Hybrid");
    println!();

    run_app(Box::new(StyleComparisonApp))
}
