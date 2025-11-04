//! Interactive Counter App with Signal<T>
//!
//! Demonstrates fine-grained reactive state management using Signal primitives
//! in a real GUI application with buttons and visual feedback.
//!
//! ## Features:
//! - Signal<i32> for reactive counter state
//! - Copy semantics - no cloning needed
//! - Multiple independent signals working together
//! - Visual feedback for signal operations
//! - Real-time subscription notifications

use flui_app::run_app;
use flui_core::{BuildContext, IntoWidget, State, StatefulWidget, Widget, Signal};
use flui_widgets::prelude::*;
use flui_types::styling::BoxShadow;
use std::sync::Arc;

// ============================================================================
// Counter App (Stateful with Signals)
// ============================================================================

/// Root app widget demonstrating Signal-based reactive state
#[derive(Debug, Clone)]
struct CounterApp;

flui_core::impl_into_widget!(CounterApp, stateful);

impl StatefulWidget for CounterApp {
    fn create_state(&self) -> Box<dyn State> {
        Box::new(CounterAppState::new())
    }
}

// ============================================================================
// App State with Signals
// ============================================================================

/// State for the counter app using Signal primitives
#[derive(Debug)]
struct CounterAppState {
    /// Main counter - Signal is Copy (8 bytes)!
    count: Signal<i32>,

    /// Operation counter - tracks how many operations performed
    operations: Signal<i32>,

    /// Last operation description
    last_operation: Signal<String>,
}

impl CounterAppState {
    fn new() -> Self {
        let count = Signal::new(0);
        let operations = Signal::new(0);
        let last_operation = Signal::new("Ready".to_string());

        // Subscribe to count changes for notifications
        let ops = operations;
        count.subscribe(Arc::new(move || {
            ops.update(|v| *v += 1);
            println!("[Signal Notification] Count changed!");
        }));

        Self {
            count,
            operations,
            last_operation,
        }
    }

    fn increment(&self) {
        self.count.increment();
        self.last_operation.set("Increment (+1)".to_string());
    }

    fn decrement(&self) {
        self.count.decrement();
        self.last_operation.set("Decrement (-1)".to_string());
    }

    fn add_ten(&self) {
        self.count.update(|v| *v += 10);
        self.last_operation.set("Add Ten (+10)".to_string());
    }

    fn subtract_ten(&self) {
        self.count.update(|v| *v -= 10);
        self.last_operation.set("Subtract Ten (-10)".to_string());
    }

    fn reset(&self) {
        self.count.set(0);
        self.last_operation.set("Reset to 0".to_string());
    }

    fn double(&self) {
        self.count.update(|v| *v *= 2);
        self.last_operation.set("Double (×2)".to_string());
    }
}

impl State for CounterAppState {
    fn build(&mut self, _ctx: &BuildContext) -> Widget {
        // Get current signal values
        let count_value = self.count.get();
        let ops_value = self.operations.get();
        let last_op = self.last_operation.get();

        // Determine color based on count value
        let count_color = if count_value > 0 {
            Color::rgb(76, 175, 80)  // Green for positive
        } else if count_value < 0 {
            Color::rgb(244, 67, 54)  // Red for negative
        } else {
            Color::rgb(33, 150, 243)  // Blue for zero
        };

        Container::builder()
            .padding(EdgeInsets::all(16.0))
            .color(Color::rgb(250, 250, 250))
            .child(
                Column::builder()
                    .main_axis_alignment(MainAxisAlignment::Center)
                    .cross_axis_alignment(CrossAxisAlignment::Center)
                    .children(vec![
                        // Title
                        Text::builder()
                            .data("🚀 Signal Counter")
                            .size(24.0)
                            .color(Color::rgb(33, 33, 33))
                            .build()
                            .into(),

                        SizedBox::builder().height(8.0).build().into(),

                        // Signal info badge
                        Container::builder()
                            .padding(EdgeInsets::symmetric(16.0, 8.0))
                            .decoration(BoxDecoration {
                                color: Some(Color::rgb(232, 245, 233)),
                                border_radius: Some(BorderRadius::circular(20.0)),
                                ..Default::default()
                            })
                            .child(
                                Text::builder()
                                    .data("⚡ Powered by Signal<T>")
                                    .size(14.0)
                                    .color(Color::rgb(76, 175, 80))
                                    .build(),
                            )
                            .build()
                            .into(),

                        SizedBox::builder().height(12.0).build().into(),

                        // Counter display
                        Container::builder()
                            .padding(EdgeInsets::all(20.0))
                            .decoration(BoxDecoration {
                                color: Some(Color::WHITE),
                                border_radius: Some(BorderRadius::circular(16.0)),
                                box_shadow: Some(vec![
                                    BoxShadow {
                                        color: Color::rgba(0, 0, 0, 25),  // 10% alpha = 25/255
                                        offset: Offset::new(0.0, 4.0),
                                        blur_radius: 12.0,
                                        spread_radius: 0.0,
                                        inset: false,
                                    },
                                ]),
                                ..Default::default()
                            })
                            .child(
                                Column::builder()
                                    .cross_axis_alignment(CrossAxisAlignment::Center)
                                    .children(vec![
                                        Text::builder()
                                            .data("Count")
                                            .size(18.0)
                                            .color(Color::rgb(117, 117, 117))
                                            .build()
                                            .into(),

                                        SizedBox::builder().height(10.0).build().into(),

                                        Text::builder()
                                            .data(format!("{}", count_value))
                                            .size(48.0)
                                            .color(count_color)
                                            .build()
                                            .into(),
                                    ])
                                    .build(),
                            )
                            .build()
                            .into(),

                        SizedBox::builder().height(12.0).build().into(),

                        // Button grid - basic operations
                        Row::builder()
                            .main_axis_alignment(MainAxisAlignment::Center)
                            .children(vec![
                                self.build_button("-", Color::rgb(255, 152, 0), {
                                    let state = CounterAppState {
                                        count: self.count,
                                        operations: self.operations,
                                        last_operation: self.last_operation,
                                    };
                                    move || state.decrement()
                                }),

                                SizedBox::builder().width(16.0).build().into(),

                                self.build_button("Reset", Color::rgb(96, 125, 139), {
                                    let state = CounterAppState {
                                        count: self.count,
                                        operations: self.operations,
                                        last_operation: self.last_operation,
                                    };
                                    move || state.reset()
                                }),

                                SizedBox::builder().width(16.0).build().into(),

                                self.build_button("+", Color::rgb(76, 175, 80), {
                                    let state = CounterAppState {
                                        count: self.count,
                                        operations: self.operations,
                                        last_operation: self.last_operation,
                                    };
                                    move || state.increment()
                                }),
                            ])
                            .build()
                            .into(),

                        SizedBox::builder().height(8.0).build().into(),

                        // Advanced operations
                        Row::builder()
                            .main_axis_alignment(MainAxisAlignment::Center)
                            .children(vec![
                                self.build_button("-10", Color::rgb(244, 67, 54), {
                                    let state = CounterAppState {
                                        count: self.count,
                                        operations: self.operations,
                                        last_operation: self.last_operation,
                                    };
                                    move || state.subtract_ten()
                                }),

                                SizedBox::builder().width(16.0).build().into(),

                                self.build_button("×2", Color::rgb(156, 39, 176), {
                                    let state = CounterAppState {
                                        count: self.count,
                                        operations: self.operations,
                                        last_operation: self.last_operation,
                                    };
                                    move || state.double()
                                }),

                                SizedBox::builder().width(16.0).build().into(),

                                self.build_button("+10", Color::rgb(33, 150, 243), {
                                    let state = CounterAppState {
                                        count: self.count,
                                        operations: self.operations,
                                        last_operation: self.last_operation,
                                    };
                                    move || state.add_ten()
                                }),
                            ])
                            .build()
                            .into(),

                        SizedBox::builder().height(12.0).build().into(),

                        // Status info
                        Container::builder()
                            .padding(EdgeInsets::all(12.0))
                            .decoration(BoxDecoration {
                                color: Some(Color::rgb(227, 242, 253)),
                                border_radius: Some(BorderRadius::circular(12.0)),
                                ..Default::default()
                            })
                            .child(
                                Column::builder()
                                    .cross_axis_alignment(CrossAxisAlignment::Start)
                                    .children(vec![
                                        Row::builder()
                                            .children(vec![
                                                Text::builder()
                                                    .data("Last Operation: ")
                                                    .size(14.0)
                                                    .color(Color::rgb(66, 66, 66))
                                                    .build()
                                                    .into(),

                                                Text::builder()
                                                    .data(last_op)
                                                    .size(14.0)
                                                    .color(Color::rgb(33, 150, 243))
                                                    .build()
                                                    .into(),
                                            ])
                                            .build()
                                            .into(),

                                        SizedBox::builder().height(8.0).build().into(),

                                        Row::builder()
                                            .children(vec![
                                                Text::builder()
                                                    .data("Total Operations: ")
                                                    .size(14.0)
                                                    .color(Color::rgb(66, 66, 66))
                                                    .build()
                                                    .into(),

                                                Text::builder()
                                                    .data(format!("{}", ops_value))
                                                    .size(14.0)
                                                    .color(Color::rgb(33, 150, 243))
                                                    .build()
                                                    .into(),
                                            ])
                                            .build()
                                            .into(),
                                    ])
                                    .build(),
                            )
                            .build()
                            .into(),

                    ])
                    .build(),
            )
            .build()
            .into()
    }

    fn init_state(&mut self, _ctx: &BuildContext) {
        println!("✅ CounterApp initialized with Signal<T>");
        println!("   Signals are Copy - 8 bytes each");
        println!("   Watch for subscription notifications!");
    }

    fn dispose(&mut self) {
        println!("❌ CounterApp disposed");
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}

impl CounterAppState {
    fn build_button<F>(&self, label: &str, color: Color, on_click: F) -> Widget
    where
        F: Fn() + 'static,
    {
        Container::builder()
            .padding(EdgeInsets::symmetric(24.0, 16.0))
            .decoration(BoxDecoration {
                color: Some(color),
                border_radius: Some(BorderRadius::circular(8.0)),
                box_shadow: Some(vec![
                    BoxShadow {
                        color: Color::rgba(0, 0, 0, 51),  // 20% alpha = 51/255
                        offset: Offset::new(0.0, 2.0),
                        blur_radius: 4.0,
                        spread_radius: 0.0,
                        inset: false,
                    },
                ]),
                ..Default::default()
            })
            .child(
                Text::builder()
                    .data(label)
                    .size(18.0)
                    .color(Color::WHITE)
                    .build(),
            )
            .build()
            .into()
    }
}

// ============================================================================
// Main Entry Point
// ============================================================================

fn main() -> Result<(), eframe::Error> {
    println!("╔══════════════════════════════════════╗");
    println!("║   Signal Counter - Flui Framework   ║");
    println!("╚══════════════════════════════════════╝");
    println!();
    println!("This example demonstrates Signal<T> reactive primitives");
    println!("in a real GUI application.");
    println!();
    println!("📋 Features:");
    println!("   ✓ Signal<i32> for reactive state");
    println!("   ✓ Copy semantics (8 bytes each)");
    println!("   ✓ No .clone() in event handlers");
    println!("   ✓ Automatic subscriptions");
    println!("   ✓ Real-time notifications");
    println!();
    println!("🎮 Try the buttons:");
    println!("   • Basic: +, -, Reset");
    println!("   • Advanced: +10, -10, ×2");
    println!();
    println!("👀 Watch the console for signal notifications!");
    println!();
    println!("🚀 Starting app...");
    println!();

    run_app(CounterApp.into_widget())
}
