//! Counter Demo - FLUI application with declarative Text widget
//!
//! Demonstrates declarative UI using the Text widget and run_app().

use flui_app::{
    run_app_with_config, AppConfig, BuildContext, ElementBase, StatelessElement, StatelessView,
    View,
};
use flui_types::styling::Color;
use flui_widgets::Text;

/// Hello World application
#[derive(Clone)]
struct HelloWorld;

impl StatelessView for HelloWorld {
    fn build(&self, _ctx: &dyn BuildContext) -> Box<dyn View> {
        Box::new(
            Text::new("Hello, World!")
                .font_size(72.0)
                .color(Color::WHITE),
        )
    }
}

impl View for HelloWorld {
    fn create_element(&self) -> Box<dyn ElementBase> {
        Box::new(StatelessElement::new(self))
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

fn main() {
    // Create app configuration
    let config = AppConfig::new()
        .with_title("FLUI Hello World - Declarative")
        .with_size(800, 600);

    // Run declarative app
    run_app_with_config(HelloWorld, config);
}
