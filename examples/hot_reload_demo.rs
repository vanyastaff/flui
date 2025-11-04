//! Hot Reload Demo
//!
//! Demonstrates the reassemble mechanism for Flutter-style hot reload.
//!
//! ## How to Test:
//!
//! 1. Run: `cargo run --example hot_reload_demo`
//! 2. The app shows a counter with a reassemble count
//! 3. Manual trigger: Call `reassemble_tree()` from code (see comments below)
//! 4. Future: Press 'R' key or file watcher will trigger automatically
//!
//! ## What This Demonstrates:
//!
//! - `State::reassemble()` lifecycle hook
//! - State persistence across reassemble (counter value stays)
//! - Reassemble count increments to show it was called
//! - Automatic rebuild after reassemble

use flui_app::run_app;
use flui_core::{BuildContext, IntoWidget, State, StatefulWidget, Widget};
use flui_widgets::prelude::*;

// ============================================================================
// Hot Reload Demo App (Stateful)
// ============================================================================

/// Root app widget demonstrating hot reload
#[derive(Debug, Clone)]
struct HotReloadApp;

flui_core::impl_into_widget!(HotReloadApp, stateful);

impl StatefulWidget for HotReloadApp {
    fn create_state(&self) -> Box<dyn State> {
        Box::new(HotReloadAppState {
            counter: 0,
            reassemble_count: 0,
        })
    }
}

// ============================================================================
// App State
// ============================================================================

/// State for the hot reload demo
#[derive(Debug)]
struct HotReloadAppState {
    /// Counter value (persists across hot reload)
    counter: i32,
    /// Number of times reassemble was called
    reassemble_count: usize,
}

impl State for HotReloadAppState {
    fn build(&mut self, _ctx: &BuildContext) -> Widget {
        // Build UI showing the counter and reassemble count
        Container::builder()
            .padding(EdgeInsets::all(40.0))
            .color(Color::rgb(250, 250, 250))
            .child(
                Column::builder()
                    .main_axis_alignment(MainAxisAlignment::Center)
                    .cross_axis_alignment(CrossAxisAlignment::Center)
                    .children(vec![
                        // Title
                        Text::builder()
                            .data("🔥 Hot Reload Demo")
                            .size(32.0)
                            .color(Color::rgb(33, 33, 33))
                            .build()
                            .into(),

                        SizedBox::builder().height(40.0).build().into(),

                        // Counter display
                        Container::builder()
                            .padding(EdgeInsets::all(20.0))
                            .decoration(BoxDecoration {
                                color: Some(Color::rgb(227, 242, 253)),
                                border_radius: Some(BorderRadius::circular(12.0)),
                                ..Default::default()
                            })
                            .child(
                                Text::builder()
                                    .data(format!("Counter: {}", self.counter))
                                    .size(48.0)
                                    .color(Color::rgb(33, 150, 243))
                                    .build(),
                            )
                            .build()
                            .into(),

                        SizedBox::builder().height(20.0).build().into(),

                        // Reassemble count
                        Container::builder()
                            .padding(EdgeInsets::all(16.0))
                            .decoration(BoxDecoration {
                                color: Some(Color::rgb(232, 245, 233)),
                                border_radius: Some(BorderRadius::circular(8.0)),
                                ..Default::default()
                            })
                            .child(
                                Text::builder()
                                    .data(format!("Reassembles: {}", self.reassemble_count))
                                    .size(24.0)
                                    .color(Color::rgb(76, 175, 80))
                                    .build(),
                            )
                            .build()
                            .into(),

                        SizedBox::builder().height(40.0).build().into(),

                        // Status indicator
                        if self.reassemble_count > 0 {
                            Container::builder()
                                .padding(EdgeInsets::symmetric(12.0, 8.0))
                                .decoration(BoxDecoration {
                                    color: Some(Color::rgb(76, 175, 80)),
                                    border_radius: Some(BorderRadius::circular(16.0)),
                                    ..Default::default()
                                })
                                .child(
                                    Text::builder()
                                        .data("✓ Hot Reload Active")
                                        .size(16.0)
                                        .color(Color::WHITE)
                                        .build(),
                                )
                                .build()
                                .into()
                        } else {
                            Container::builder()
                                .padding(EdgeInsets::symmetric(12.0, 8.0))
                                .decoration(BoxDecoration {
                                    color: Some(Color::rgb(189, 189, 189)),
                                    border_radius: Some(BorderRadius::circular(16.0)),
                                    ..Default::default()
                                })
                                .child(
                                    Text::builder()
                                        .data("Waiting for hot reload...")
                                        .size(16.0)
                                        .color(Color::WHITE)
                                        .build(),
                                )
                                .build()
                                .into()
                        },

                        SizedBox::builder().height(40.0).build().into(),

                        // Instructions box
                        Container::builder()
                            .padding(EdgeInsets::all(20.0))
                            .decoration(BoxDecoration {
                                color: Some(Color::rgb(255, 243, 224)),
                                border_radius: Some(BorderRadius::circular(8.0)),
                                ..Default::default()
                            })
                            .child(
                                Column::builder()
                                    .cross_axis_alignment(CrossAxisAlignment::Start)
                                    .children(vec![
                                        Text::builder()
                                            .data("📝 Instructions")
                                            .size(18.0)
                                            .color(Color::rgb(230, 81, 0))
                                            .build()
                                            .into(),

                                        SizedBox::builder().height(12.0).build().into(),

                                        Text::builder()
                                            .data("1. To trigger hot reload manually:")
                                            .size(14.0)
                                            .color(Color::rgb(62, 39, 35))
                                            .build()
                                            .into(),

                                        SizedBox::builder().height(4.0).build().into(),

                                        Text::builder()
                                            .data("   Call pipeline.reassemble_tree() in FluiApp")
                                            .size(14.0)
                                            .color(Color::rgb(117, 117, 117))
                                            .build()
                                            .into(),

                                        SizedBox::builder().height(8.0).build().into(),

                                        Text::builder()
                                            .data("2. Watch reassemble count increment")
                                            .size(14.0)
                                            .color(Color::rgb(62, 39, 35))
                                            .build()
                                            .into(),

                                        SizedBox::builder().height(8.0).build().into(),

                                        Text::builder()
                                            .data("3. Note: Counter value persists across reassemble!")
                                            .size(14.0)
                                            .color(Color::rgb(62, 39, 35))
                                            .build()
                                            .into(),

                                        SizedBox::builder().height(12.0).build().into(),

                                        Text::builder()
                                            .data("⚠️  State fields persist (Rust limitation)")
                                            .size(12.0)
                                            .color(Color::rgb(117, 117, 117))
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
    }

    fn init_state(&mut self, _ctx: &BuildContext) {
        println!("✅ HotReloadApp state initialized");
    }

    fn did_update_widget(&mut self, _old_widget: &dyn StatefulWidget, _ctx: &BuildContext) {
        // Called when widget config changes
    }

    fn dispose(&mut self) {
        println!("❌ HotReloadApp state disposed");
    }

    fn reassemble(&mut self) {
        // This is called during hot reload!
        self.reassemble_count += 1;

        println!();
        println!("╔══════════════════════════════════════╗");
        println!("║       🔥 HOT RELOAD TRIGGERED!       ║");
        println!("╚══════════════════════════════════════╝");
        println!("   Reassemble count: {}", self.reassemble_count);
        println!("   Counter value: {} (persisted!)", self.counter);
        println!();

        // Clear any caches here if needed
        // self.cached_data = None;

        // Note: Widget is automatically marked dirty and will rebuild
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}

// ============================================================================
// Main Entry Point
// ============================================================================

fn main() -> Result<(), eframe::Error> {
    println!("╔══════════════════════════════════════╗");
    println!("║  Hot Reload Demo - Flui Framework   ║");
    println!("╚══════════════════════════════════════╝");
    println!();
    println!("This example demonstrates the reassemble lifecycle hook.");
    println!();
    println!("📋 Features:");
    println!("   ✓ State::reassemble() lifecycle method");
    println!("   ✓ State persistence across hot reload");
    println!("   ✓ Automatic rebuild after reassemble");
    println!("   ✓ Reassemble count tracking");
    println!();
    println!("🔧 How to trigger hot reload:");
    println!("   1. Manual: Modify FluiApp::update() to call:");
    println!("      self.pipeline.reassemble_tree();");
    println!("   2. TODO: Add keyboard shortcut (Ctrl+R)");
    println!("   3. TODO: Add file watcher integration");
    println!();
    println!("💡 What happens during hot reload:");
    println!("   1. reassemble_tree() walks entire element tree");
    println!("   2. For each StatefulElement, calls state.reassemble()");
    println!("   3. State can clear caches, refresh computed values");
    println!("   4. Elements marked dirty → framework rebuilds UI");
    println!("   5. State fields persist (counter value stays same!)");
    println!();
    println!("🚀 Starting app...");
    println!();

    run_app(HotReloadApp.into_widget())
}
