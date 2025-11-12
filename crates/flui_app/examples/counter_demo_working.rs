//! Simple working Counter Demo for FLUI
//!
//! This is a simplified version that works with the current API.
//!
//! # Building
//!
//! ## Desktop
//! ```bash
//! cargo run -p flui_app --example counter_demo_working --release
//! ```

use flui_app::run_app;
use flui_core::prelude::*;
use flui_core::hooks::use_signal;
use flui_widgets::prelude::*;

#[derive(Debug, Clone)]
struct CounterApp;

impl View for CounterApp {
    fn build(self, ctx: &BuildContext) -> impl IntoElement {
        let count = use_signal(ctx, 0);

        // Get current count value using with_hook_context_mut
        let count_value = ctx.with_hook_context_mut(|hook_ctx| count.get(hook_ctx));

        // Simple column layout with counter and buttons
        Container::builder()
            .color(Color::rgb(245, 245, 250))
            .width(800.0)
            .height(600.0)
            .padding(EdgeInsets::all(32.0))
            .child(
                Column::builder()
                    .main_axis_alignment(MainAxisAlignment::Center)
                    .cross_axis_alignment(CrossAxisAlignment::Center)
                    .child(
                        Text::builder()
                            .data("FLUI Counter")
                            .size(48.0)
                            .color(Color::rgb(30, 30, 30))
                            .build()
                    )
                    .child(
                        Text::builder()
                            .data(format!("Count: {}", count_value))
                            .size(72.0)
                            .color(Color::rgb(100, 150, 255))
                            .build()
                    )
                    .child(
                        Row::builder()
                            .main_axis_alignment(MainAxisAlignment::Center)
                            .child(
                                Button::builder("−")
                                    .color(Color::rgb(255, 100, 100))
                                    .padding(EdgeInsets::all(16.0))
                                    .on_tap(Box::new(move || {
                                        count.update_mut(|n| {
                                            if *n > 0 {
                                                *n -= 1;
                                            }
                                        });
                                    }))
                                    .build()
                            )
                            .child(
                                Button::builder("Reset")
                                    .color(Color::rgb(150, 150, 150))
                                    .padding(EdgeInsets::all(16.0))
                                    .on_tap(Box::new(move || {
                                        count.set(0);
                                    }))
                                    .build()
                            )
                            .child(
                                Button::builder("+")
                                    .color(Color::rgb(100, 255, 100))
                                    .padding(EdgeInsets::all(16.0))
                                    .on_tap(Box::new(move || {
                                        count.update_mut(|n| *n += 1);
                                    }))
                                    .build()
                            )
                            .build()
                    )
                    .child(
                        Text::builder()
                            .data("Built with FLUI")
                            .size(14.0)
                            .color(Color::rgba(30, 30, 30, 128))
                            .build()
                    )
                    .build()
            )
            .build()
    }
}

/// Desktop entry point
#[cfg(not(any(target_os = "android", target_os = "ios", target_arch = "wasm32")))]
fn main() {
    // Initialize logging
    use tracing_subscriber::{fmt, prelude::*, EnvFilter};

    tracing_subscriber::registry()
        .with(fmt::layer())
        .with(EnvFilter::from_default_env().add_directive(tracing::Level::INFO.into()))
        .init();

    tracing::info!("Starting FLUI Counter on Desktop");
    run_app(CounterApp);
}

/// Android entry point
#[cfg(target_os = "android")]
#[no_mangle]
fn android_main(app: winit::platform::android::activity::AndroidApp) {
    use android_logger::Config;

    // Initialize Android logging
    android_logger::init_once(
        Config::default()
            .with_max_level(log::LevelFilter::Info)
            .with_tag("FLUI")
    );

    tracing::info!("Starting FLUI Counter on Android");
    run_app(CounterApp);
}

/// Web (WebAssembly) entry point
#[cfg(target_arch = "wasm32")]
use wasm_bindgen::prelude::*;

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen(start)]
pub fn wasm_main() {
    // Setup panic hook for better error messages
    console_error_panic_hook::set_once();

    // Initialize logging to browser console
    wasm_logger::init(wasm_logger::Config::default());

    tracing::info!("Starting FLUI Counter on Web");
    run_app(CounterApp);
}
