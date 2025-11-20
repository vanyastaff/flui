use flui_app::run_app;
use flui_core::prelude::*;
use flui_widgets::*;
use flui_types::{Color, EdgeInsets};
use flui_types::layout::{MainAxisAlignment, CrossAxisAlignment};

fn main() {
    run_app(CounterApp);
}

#[derive(Debug, Clone)]
struct CounterApp;

impl View for CounterApp {
    fn build(self, ctx: &BuildContext) -> impl IntoElement {
        // Use hooks for reactive state management
        let count = use_signal(ctx, 0);

        // Read current value for display
        let current_value = count.get();
        tracing::info!("Building UI with current count: {}", current_value);

        Container::builder()
            .color(Color::rgb(240, 240, 240))
            .padding(EdgeInsets::all(40.0))
            .child(
                Column::builder()
                    .main_axis_alignment(MainAxisAlignment::Center)
                    .cross_axis_alignment(CrossAxisAlignment::Center)
                    .child(
                        Text::builder()
                            .data("FLUI Counter Demo")
                            .size(32.0)
                            .color(Color::rgb(50, 50, 50))
                            .build(),
                    )
                    .child(
                        Container::builder()
                            .padding(EdgeInsets::all(20.0))
                            .child(
                                Text::builder()
                                    .data(format!("Count: {}", current_value))
                                    .size(48.0)
                                    .color(Color::rgb(0, 120, 200))
                                    .build(),
                            )
                            .build(),
                    )
                    .child(Button::builder("Increment")
                        .on_tap(move || {
                            tracing::info!("🔵 BUTTON CLICKED! Updating count...");
                            count.update_mut(|c| {
                                *c += 1;
                                tracing::info!("🔵 Count updated to: {}", *c);
                            });
                        })
                        .build()
                    )
                    .build()
            ).build()
    }
}
